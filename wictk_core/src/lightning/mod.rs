use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use geo::Point;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::geo_point::GeoPoint;

pub const YR_LIGHTNING_URL: &str = "https://www.yr.no/api/v0/lightning-events?fromHours=24";

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

    pub async fn fetch(client: &Client, url: &str) -> Result<Vec<Lightning>> {
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch lightning data from {url}"))?
            .error_for_status()
            .with_context(|| format!("Lightning provider returned an error from {url}"))?
            .json::<LightningResponse>()
            .await
            .with_context(|| format!("Failed to decode lightning response from {url}"))?;

        parse_historical_data(response.historical_data)
    }
}

#[derive(Deserialize)]
struct LightningResponse {
    #[serde(rename = "historicalData")]
    historical_data: Value,
}

/// Raw event on the form `[timestamp, longitude, latitude, magic_value]`.
type RawEvent = (i64, f64, f64, u8);

/// `historicalData` is a JSON array, usually embedded as a string.
fn parse_historical_data(data: Value) -> Result<Vec<Lightning>> {
    let events = match data {
        Value::String(embedded) => serde_json::from_str::<Vec<Value>>(&embedded)
            .context("Failed to parse embedded 'historicalData' payload")?,
        Value::Array(events) => events,
        other => bail!("Expected 'historicalData' to be an array, got {other}"),
    };

    Ok(events.into_iter().filter_map(parse_event).collect())
}

fn parse_event(event: Value) -> Option<Lightning> {
    let (timestamp, longitude, latitude, magic_value) =
        serde_json::from_value::<RawEvent>(event).ok()?;
    let time = DateTime::<Utc>::from_timestamp(timestamp, 0)?;
    Some(Lightning::new(
        Point::new(longitude, latitude),
        time,
        magic_value,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, TestResult};
    use serde_json::json;

    quickcheck! {
        fn prop_embedded_and_inline_arrays_parse_identically(events: Vec<(u32, i16, i16, u8)>) -> bool {
            let events: Vec<Value> = events
                .into_iter()
                .map(|(ts, lon, lat, magic)| json!([ts, lon as f64 / 200.0, lat as f64 / 400.0, magic]))
                .collect();
            let inline = parse_historical_data(Value::Array(events.clone())).unwrap();
            let embedded = parse_historical_data(Value::String(Value::Array(events.clone()).to_string())).unwrap();

            inline.len() == events.len()
                && inline.iter().zip(&embedded).all(|(a, b)| a.location == b.location && a.time == b.time && a.magic_value == b.magic_value)
        }

        fn prop_malformed_events_are_skipped(valid: Vec<(u32, u8)>, garbage: Vec<String>) -> TestResult {
            let events: Vec<Value> = valid
                .iter()
                .map(|(ts, magic)| json!([ts, 10.0, 60.0, magic]))
                .chain(garbage.into_iter().map(Value::String))
                .collect();

            let parsed = parse_historical_data(Value::Array(events)).unwrap();

            TestResult::from_bool(parsed.len() == valid.len())
        }
    }

    #[test]
    fn rejects_magic_value_overflow_instead_of_truncating() {
        let parsed = parse_historical_data(json!([[1_700_000_000, 10.0, 60.0, 300]])).unwrap();

        assert!(parsed.is_empty());
    }

    #[test]
    fn rejects_non_array_payload() {
        assert!(parse_historical_data(json!({ "not": "an array" })).is_err());
    }

    #[tokio::test]
    async fn fetches_lightning() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock_response = json!({
            "historicalData": "[[1700000000,10.7522,59.9139,1],[1700000060,11.3001,60.1699,2]]"
        });

        let _m = server
            .mock("GET", "/api/v0/lightning-events")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response.to_string())
            .create_async()
            .await;

        let url = format!("{}/api/v0/lightning-events", server.url());
        let lightning_data = Lightning::fetch(&Client::new(), &url).await?;

        assert_eq!(lightning_data.len(), 2);
        assert_eq!(lightning_data[0].location, Point::new(10.7522, 59.9139));
        assert_eq!(lightning_data[0].magic_value, 1);
        assert_eq!(lightning_data[1].location, Point::new(11.3001, 60.1699));
        assert_eq!(lightning_data[1].magic_value, 2);
        Ok(())
    }
}
