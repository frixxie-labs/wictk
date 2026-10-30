use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::Serialize;
use wictk_core::{Earthquake, Lightning};

use crate::api::post_json;
use crate::device::DeviceId;

/// Keeps each lightning request well below hemrs' per-request strike limit.
const LIGHTNING_BATCH_SIZE: usize = 1000;

/// A WGS 84 position as hemrs expects it.
#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct NewLightningStrike {
    pub timestamp: DateTime<Utc>,
    pub device: DeviceId,
    pub coordinates: Coordinates,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct NewEarthquake {
    pub timestamp: DateTime<Utc>,
    pub device: DeviceId,
    pub coordinates: Coordinates,
    pub depth_km: f32,
    pub magnitude: f32,
    pub significance: i32,
    pub tsunami: bool,
}

pub fn lightning_strike(lightning: &Lightning, device: DeviceId) -> NewLightningStrike {
    NewLightningStrike {
        timestamp: lightning.time,
        device,
        coordinates: Coordinates {
            latitude: lightning.location.y(),
            longitude: lightning.location.x(),
        },
    }
}

/// hemrs requires magnitude and significance, so earthquakes lacking either yield `None`.
pub fn earthquake(earthquake: &Earthquake, device: DeviceId) -> Option<NewEarthquake> {
    Some(NewEarthquake {
        timestamp: earthquake.time,
        device,
        coordinates: Coordinates {
            latitude: earthquake.coordinates.lat().into(),
            longitude: earthquake.coordinates.lon().into(),
        },
        depth_km: earthquake.depth_km as f32,
        magnitude: earthquake.magnitude? as f32,
        significance: i32::try_from(earthquake.significance?).ok()?,
        tsunami: earthquake.tsunami,
    })
}

/// Writes lightning strikes and earthquakes to hemrs.
pub struct GeoEventClient {
    client: Client,
    lightning_url: Url,
    earthquake_url: Url,
}

impl GeoEventClient {
    pub fn new(client: Client, lightning_url: Url, earthquake_url: Url) -> Self {
        Self {
            client,
            lightning_url,
            earthquake_url,
        }
    }

    pub async fn store_lightning(&self, strikes: &[NewLightningStrike]) -> Result<()> {
        for batch in strikes.chunks(LIGHTNING_BATCH_SIZE) {
            post_json(&self.client, self.lightning_url.clone(), batch).await?;
        }
        Ok(())
    }

    pub async fn store_earthquake(&self, earthquake: &NewEarthquake) -> Result<()> {
        post_json(&self.client, self.earthquake_url.clone(), earthquake).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use geo::Point;
    use quickcheck::quickcheck;
    use wictk_core::EarthquakeStatus;

    fn time(secs: u32) -> DateTime<Utc> {
        Utc.timestamp_opt(secs as i64, 0).unwrap()
    }

    fn quake(magnitude: Option<f64>, significance: Option<u64>) -> Earthquake {
        Earthquake {
            id: "id".to_string(),
            magnitude,
            place: None,
            time: time(0),
            updated: time(0),
            coordinates: wictk_core::Coordinates::new(138.25, 36.5).unwrap(),
            depth_km: 10.0,
            significance,
            alert_level: None,
            status: EarthquakeStatus::Reviewed,
            tsunami: true,
            details_url: String::new(),
        }
    }

    fn client(server: &mockito::Server) -> GeoEventClient {
        let base: Url = server.url().parse().unwrap();
        GeoEventClient::new(
            Client::new(),
            base.join("lightning").unwrap(),
            base.join("earthquakes").unwrap(),
        )
    }

    quickcheck! {
        fn prop_lightning_strike_preserves_time_and_position(secs: u32, lon: i16, lat: i16) -> bool {
            let point = Point::new(lon as f64 / 200.0, lat as f64 / 400.0);
            let strike = lightning_strike(&Lightning::new(point, time(secs), 1), DeviceId(3));

            strike.timestamp == time(secs)
                && strike.device == DeviceId(3)
                && strike.coordinates == Coordinates { latitude: point.y(), longitude: point.x() }
        }

        fn prop_earthquake_requires_magnitude_and_significance(magnitude: Option<u8>, significance: Option<u16>) -> bool {
            let result = earthquake(&quake(magnitude.map(f64::from), significance.map(u64::from)), DeviceId(1));
            result.is_some() == (magnitude.is_some() && significance.is_some())
        }
    }

    #[test]
    fn serializes_earthquake_in_hemrs_format() {
        let new = earthquake(&quake(Some(5.5), Some(400)), DeviceId(2)).unwrap();

        assert_eq!(
            serde_json::to_value(new).unwrap(),
            serde_json::json!({
                "timestamp": "1970-01-01T00:00:00Z",
                "device": 2,
                "coordinates": { "latitude": 36.5, "longitude": 138.25 },
                "depth_km": 10.0,
                "magnitude": 5.5,
                "significance": 400,
                "tsunami": true
            })
        );
    }

    #[tokio::test]
    async fn posts_lightning_in_batches() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/lightning")
            .with_status(201)
            .expect(2)
            .create_async()
            .await;
        let strike = lightning_strike(
            &Lightning::new(Point::new(10.0, 60.0), time(0), 1),
            DeviceId(1),
        );

        client(&server)
            .store_lightning(&vec![strike; LIGHTNING_BATCH_SIZE + 1])
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn skips_posting_when_there_is_no_lightning() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/lightning")
            .expect(0)
            .create_async()
            .await;

        client(&server).store_lightning(&[]).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn reports_status_of_earthquake_posts() {
        for (status, succeeds) in [(200, true), (201, true), (400, false), (500, false)] {
            let mut server = mockito::Server::new_async().await;
            let _mock = server
                .mock("POST", "/earthquakes")
                .with_status(status)
                .create_async()
                .await;
            let new = earthquake(&quake(Some(5.5), Some(400)), DeviceId(2)).unwrap();

            let result = client(&server).store_earthquake(&new).await;

            assert_eq!(result.is_ok(), succeeds, "{status}");
        }
    }
}
