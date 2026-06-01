use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use geo::Point;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// A geographic point with longitude (x) and latitude (y)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GeoPoint {
    /// Longitude
    pub x: f64,
    /// Latitude
    pub y: f64,
}

impl From<Point> for GeoPoint {
    fn from(p: Point) -> Self {
        GeoPoint { x: p.x(), y: p.y() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Lightning {
    #[schema(value_type = GeoPoint)]
    pub location: Point,
    pub time: DateTime<Utc>,
    pub magic_value: u8,
}

impl Lightning {
    pub fn new(location: Point, time: DateTime<Utc>, magic_value: u8) -> Self {
        Lightning {
            location,
            time,
            magic_value,
        }
    }

    pub async fn find_ligntning(client: &Client, url: &str) -> Result<Vec<Lightning>> {
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch lightning data from {url}"))?
            .json::<Value>()
            .await
            .with_context(|| format!("Failed to decode lightning response from {url}"))?;
        let response_string = response
            .get("historicalData")
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid response format: 'historicalData' field is missing")
            })?
            .to_string();

        let data = response_string.trim_matches('"');

        let lightning_data: Value = serde_json::from_str(data)
            .with_context(|| format!("Failed to parse 'historicalData' payload from {url}"))?;

        let lightning_data = lightning_data
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Expected an array of lightning events"))?
            .iter()
            .filter_map(|event| {
                // event is on the form: [timestamp, longitude, latitude, magic_value]
                match event.as_array() {
                    Some(arr) if arr.len() == 4 => {
                        let timestamp = arr[0].as_i64()?;
                        let longitude = arr[1].as_f64()?;
                        let latitude = arr[2].as_f64()?;
                        let magic_value = arr[3].as_u64()? as u8;

                        let time = DateTime::<Utc>::from_timestamp(timestamp, 0);
                        match time {
                            Some(time) => {
                                let location = Point::new(longitude, latitude);
                                Some(Lightning::new(location, time, magic_value))
                            }
                            None => None,
                        }
                    }
                    _ => None,
                }
            })
            .collect::<Vec<Lightning>>();
        Ok(lightning_data)
    }
}

#[cfg(test)]

mod tests {
    use super::*;
    use geo::point;

    #[test]
    fn test_lightning_creation() {
        let location = point!(x: 10.0, y: 20.0);
        let time = Utc::now();
        let magic_value = 42;

        let lightning = Lightning::new(location, time, magic_value);
        assert_eq!(lightning.location, location);
        assert_eq!(lightning.time, time);
        assert_eq!(lightning.magic_value, magic_value);
    }

    #[tokio::test]
    async fn test_find_lightning() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock_response = serde_json::json!({
            "historicalData": "[[1700000000,10.7522,59.9139,1],[1700000060,11.3001,60.1699,2]]"
        });

        let _m = server
            .mock("GET", "/api/v0/lightning-events")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response.to_string())
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/api/v0/lightning-events", server.url());
        let lightning_data = Lightning::find_ligntning(&client, &url).await?;

        assert_eq!(lightning_data.len(), 2);
        assert_eq!(lightning_data[0].location.x(), 10.7522);
        assert_eq!(lightning_data[0].location.y(), 59.9139);
        assert_eq!(lightning_data[0].magic_value, 1);
        assert_eq!(lightning_data[1].location.x(), 11.3001);
        assert_eq!(lightning_data[1].location.y(), 60.1699);
        assert_eq!(lightning_data[1].magic_value, 2);
        Ok(())
    }
}
