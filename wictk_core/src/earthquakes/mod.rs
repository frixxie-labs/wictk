use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Coordinates;

pub const USGS_ALL_DAY_FEED_URL: &str =
    "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EarthquakeAlertLevel {
    Green,
    Yellow,
    Orange,
    Red,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EarthquakeStatus {
    Automatic,
    Reviewed,
    Deleted,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct Earthquake {
    pub id: String,
    pub magnitude: Option<f64>,
    pub place: Option<String>,
    pub time: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub coordinates: Coordinates,
    pub depth_km: f64,
    pub significance: Option<u64>,
    pub alert_level: Option<EarthquakeAlertLevel>,
    pub status: EarthquakeStatus,
    pub tsunami: bool,
    pub details_url: String,
}

impl Earthquake {
    pub async fn fetch_feed(client: &Client, url: &str) -> Result<Vec<Self>> {
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch earthquake data from {url}"))?
            .error_for_status()
            .with_context(|| format!("USGS earthquake feed returned an error from {url}"))?;

        let feed = response
            .json::<UsgsFeatureCollection>()
            .await
            .with_context(|| format!("Failed to decode earthquake response from {url}"))?;

        parse_feed(feed)
    }
}

#[derive(Debug, Deserialize)]
struct UsgsFeatureCollection {
    #[serde(rename = "type")]
    _type: FeatureCollectionTag,
    features: Vec<UsgsFeature>,
}

#[derive(Debug, Deserialize)]
enum FeatureCollectionTag {
    FeatureCollection,
}

#[derive(Debug, Deserialize)]
struct UsgsFeature {
    #[serde(rename = "type")]
    _type: FeatureTag,
    id: String,
    properties: UsgsProperties,
    geometry: UsgsGeometry,
}

#[derive(Debug, Deserialize)]
enum FeatureTag {
    Feature,
}

#[derive(Debug, Deserialize)]
struct UsgsProperties {
    mag: Option<f64>,
    place: Option<String>,
    time: i64,
    updated: i64,
    url: String,
    sig: Option<u64>,
    alert: Option<EarthquakeAlertLevel>,
    status: EarthquakeStatus,
    tsunami: u8,
    #[serde(rename = "type")]
    event_type: UsgsEventType,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum UsgsEventType {
    Earthquake,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum UsgsGeometry {
    /// `[longitude, latitude, depth_km]`
    Point { coordinates: (f64, f64, f64) },
}

fn parse_feed(feed: UsgsFeatureCollection) -> Result<Vec<Earthquake>> {
    feed.features
        .into_iter()
        .filter(|feature| feature.properties.event_type == UsgsEventType::Earthquake)
        .map(Earthquake::try_from)
        .collect()
}

impl TryFrom<UsgsFeature> for Earthquake {
    type Error = anyhow::Error;

    fn try_from(feature: UsgsFeature) -> Result<Self> {
        let UsgsGeometry::Point {
            coordinates: (longitude, latitude, depth_km),
        } = feature.geometry;
        if !depth_km.is_finite() {
            bail!("Event {} contains an invalid depth", feature.id);
        }
        let coordinates = Coordinates::new(longitude as f32, latitude as f32)
            .with_context(|| format!("Event {} contains invalid coordinates", feature.id))?;

        let time = DateTime::<Utc>::from_timestamp_millis(feature.properties.time)
            .with_context(|| format!("Event {} contains an invalid time", feature.id))?;
        let updated = DateTime::<Utc>::from_timestamp_millis(feature.properties.updated)
            .with_context(|| format!("Event {} contains an invalid updated time", feature.id))?;
        let tsunami = match feature.properties.tsunami {
            0 => false,
            1 => true,
            value => bail!(
                "Event {} contains invalid tsunami value {value}",
                feature.id
            ),
        };

        Ok(Self {
            id: feature.id,
            magnitude: feature.properties.mag,
            place: feature.properties.place,
            time,
            updated,
            coordinates,
            depth_km,
            significance: feature.properties.sig,
            alert_level: feature.properties.alert,
            status: feature.properties.status,
            tsunami,
            details_url: feature.properties.url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn feature(overrides: Value) -> Value {
        let mut feature = json!({
            "type": "Feature",
            "properties": {
                "mag": 4.7,
                "place": "12 km NW of Example",
                "time": 1_721_300_000_000_i64,
                "updated": 1_721_300_060_000_i64,
                "url": "https://earthquake.usgs.gov/earthquakes/eventpage/us-test",
                "sig": 340,
                "alert": "green",
                "status": "reviewed",
                "tsunami": 0,
                "type": "earthquake"
            },
            "geometry": {
                "type": "Point",
                "coordinates": [10.2, 59.1, 8.3]
            },
            "id": "us-test"
        });

        merge(&mut feature, overrides);
        feature
    }

    fn feed(feature: Value) -> Value {
        json!({
            "type": "FeatureCollection",
            "metadata": { "count": 1 },
            "features": [feature]
        })
    }

    fn merge(target: &mut Value, source: Value) {
        match (target, source) {
            (Value::Object(target), Value::Object(source)) => {
                for (key, value) in source {
                    merge(target.entry(key).or_insert(Value::Null), value);
                }
            }
            (target, source) => *target = source,
        }
    }

    fn parse(value: Value) -> Result<Vec<Earthquake>> {
        parse_feed(serde_json::from_value(value)?)
    }

    #[test]
    fn parses_geojson_event() -> Result<()> {
        let earthquakes = parse(feed(feature(json!({}))))?;

        assert_eq!(earthquakes.len(), 1);
        let earthquake = &earthquakes[0];
        assert_eq!(earthquake.id, "us-test");
        assert_eq!(earthquake.magnitude, Some(4.7));
        assert_eq!(earthquake.place.as_deref(), Some("12 km NW of Example"));
        assert_eq!(earthquake.coordinates, Coordinates::new(10.2, 59.1)?);
        assert_eq!(earthquake.depth_km, 8.3);
        assert_eq!(earthquake.significance, Some(340));
        assert_eq!(earthquake.alert_level, Some(EarthquakeAlertLevel::Green));
        assert_eq!(earthquake.status, EarthquakeStatus::Reviewed);
        assert!(!earthquake.tsunami);
        Ok(())
    }

    #[test]
    fn accepts_missing_optional_properties_and_unknown_enums() -> Result<()> {
        let mut event = feature(json!({
            "properties": { "status": "future-status" }
        }));
        let properties = event["properties"].as_object_mut().unwrap();
        for property in ["mag", "place", "sig", "alert"] {
            properties.remove(property);
        }
        let earthquakes = parse(feed(event))?;

        let earthquake = &earthquakes[0];
        assert_eq!(earthquake.magnitude, None);
        assert_eq!(earthquake.place, None);
        assert_eq!(earthquake.significance, None);
        assert_eq!(earthquake.alert_level, None);
        assert_eq!(earthquake.status, EarthquakeStatus::Unknown);
        Ok(())
    }

    #[test]
    fn rejects_malformed_geometry() {
        let result = parse(feed(feature(json!({
            "geometry": { "coordinates": [10.2, 59.1] }
        }))));

        assert!(result.is_err());
    }

    #[test]
    fn rejects_out_of_range_coordinates() {
        let result = parse(feed(feature(json!({
            "geometry": { "coordinates": [10.2, 91.0, 8.3] }
        }))));

        assert!(result.is_err());
    }

    #[test]
    fn rejects_non_feature_collection() {
        let mut value = feed(feature(json!({})));
        value["type"] = json!("Feature");

        assert!(parse(value).is_err());
    }

    #[test]
    fn ignores_non_earthquake_events() -> Result<()> {
        let earthquakes = parse(feed(feature(json!({
            "properties": { "type": "quarry blast" }
        }))))?;

        assert!(earthquakes.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_invalid_timestamp() {
        let result = parse(feed(feature(json!({
            "properties": { "time": i64::MAX }
        }))));

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetches_feed() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/all_day.geojson")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(feed(feature(json!({}))).to_string())
            .expect(1)
            .create_async()
            .await;

        let earthquakes =
            Earthquake::fetch_feed(&Client::new(), &format!("{}/all_day.geojson", server.url()))
                .await?;

        assert_eq!(earthquakes.len(), 1);
        mock.assert_async().await;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unsuccessful_response() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/all_day.geojson")
            .with_status(503)
            .create_async()
            .await;

        let result =
            Earthquake::fetch_feed(&Client::new(), &format!("{}/all_day.geojson", server.url()))
                .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_malformed_response() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/all_day.geojson")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not json")
            .create_async()
            .await;

        let result =
            Earthquake::fetch_feed(&Client::new(), &format!("{}/all_day.geojson", server.url()))
                .await;

        assert!(result.is_err());
    }
}
