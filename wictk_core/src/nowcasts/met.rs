use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::locations::Coordinates;

use super::Nowcast;

pub const MET_NOWCAST_URL: &str = "https://api.met.no/weatherapi/nowcast/2.0/complete";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MetNowcast {
    pub time: DateTime<Utc>,
    pub location: Coordinates,
    pub description: String,
    pub air_temperature: f32,
    pub relative_humidity: f32,
    pub precipitation_rate: f32,
    pub precipitation_amount: f32,
    pub wind_speed: f32,
    pub wind_speed_gust: f32,
    pub wind_from_direction: f32,
}

impl std::fmt::Display for MetNowcast {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}, {}, {}, {}, {}, {}, {}, {}, {}",
            self.time,
            self.location,
            self.description,
            self.air_temperature,
            self.relative_humidity,
            self.precipitation_rate,
            self.precipitation_amount,
            self.wind_speed,
            self.wind_speed_gust
        )
    }
}

#[derive(Deserialize)]
struct MetResponse {
    geometry: MetGeometry,
    properties: MetProperties,
}

#[derive(Deserialize)]
struct MetGeometry {
    coordinates: Vec<f32>,
}

#[derive(Deserialize)]
struct MetProperties {
    meta: MetMeta,
    timeseries: Vec<MetTimeStep>,
}

#[derive(Deserialize)]
struct MetMeta {
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct MetTimeStep {
    data: MetStepData,
}

/// Only the first time step carries the full set of values.
#[derive(Deserialize)]
struct MetStepData {
    instant: MetInstant,
    next_1_hours: Option<MetNextHour>,
}

#[derive(Deserialize)]
struct MetInstant {
    details: MetInstantDetails,
}

#[derive(Deserialize)]
struct MetInstantDetails {
    air_temperature: Option<f32>,
    relative_humidity: Option<f32>,
    precipitation_rate: Option<f32>,
    wind_speed: Option<f32>,
    wind_speed_of_gust: Option<f32>,
    wind_from_direction: Option<f32>,
}

#[derive(Deserialize)]
struct MetNextHour {
    summary: MetSummary,
    details: MetNextHourDetails,
}

#[derive(Deserialize)]
struct MetSummary {
    symbol_code: String,
}

#[derive(Deserialize)]
struct MetNextHourDetails {
    precipitation_amount: f32,
}

impl TryFrom<MetResponse> for MetNowcast {
    type Error = anyhow::Error;

    fn try_from(response: MetResponse) -> Result<Self> {
        let location = match response.geometry.coordinates.as_slice() {
            [lon, lat, ..] => Coordinates::new(*lon, *lat)?,
            _ => bail!("Expected longitude and latitude in nowcast geometry"),
        };
        let current = response
            .properties
            .timeseries
            .into_iter()
            .next()
            .context("Nowcast contains no time steps")?
            .data;
        let next_hour = current
            .next_1_hours
            .context("Nowcast is missing next_1_hours")?;
        let details = current.instant.details;

        Ok(Self {
            time: response.properties.meta.updated_at,
            location,
            description: next_hour.summary.symbol_code,
            air_temperature: details.air_temperature.context("Missing air_temperature")?,
            relative_humidity: details
                .relative_humidity
                .context("Missing relative_humidity")?,
            precipitation_rate: details
                .precipitation_rate
                .context("Missing precipitation_rate")?,
            precipitation_amount: next_hour.details.precipitation_amount,
            wind_speed: details.wind_speed.context("Missing wind_speed")?,
            wind_speed_gust: details
                .wind_speed_of_gust
                .context("Missing wind_speed_of_gust")?,
            wind_from_direction: details
                .wind_from_direction
                .context("Missing wind_from_direction")?,
        })
    }
}

impl From<MetNowcast> for Nowcast {
    fn from(met: MetNowcast) -> Self {
        Self::Met(met)
    }
}

impl MetNowcast {
    pub async fn fetch(client: &Client, url: &str, location: &Coordinates) -> Result<Nowcast> {
        let response = client
            .get(url)
            .query(&[("lat", location.lat()), ("lon", location.lon())])
            .send()
            .await
            .context("Request to Met.no failed")?
            .error_for_status()
            .context("Met.no nowcast returned an error")?
            .json::<MetResponse>()
            .await
            .context("Deserialization from Met.no failed")?;

        Ok(MetNowcast::try_from(response)?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const FIXTURE: &str = include_str!("../../tests/fixtures/met_nowcast.json");

    fn parse(value: Value) -> Result<MetNowcast> {
        MetNowcast::try_from(serde_json::from_value::<MetResponse>(value)?)
    }

    #[test]
    fn parses_nowcast() -> Result<()> {
        let met = parse(serde_json::from_str(FIXTURE)?)?;

        assert_eq!(met.location, Coordinates::new(10.4034, 63.4308)?);
        assert_eq!(met.time.to_string(), "2023-08-14 18:16:07 UTC");
        assert_eq!(met.description, "cloudy");
        assert_eq!(met.air_temperature, 17.7);
        assert_eq!(met.relative_humidity, 80.5);
        assert_eq!(met.precipitation_amount, 0.0);
        assert_eq!(met.wind_speed, 2.7);
        assert_eq!(met.wind_speed_gust, 6.1);
        assert_eq!(met.wind_from_direction, 294.4);
        Ok(())
    }

    #[test]
    fn reads_precipitation_rate_separately_from_amount() -> Result<()> {
        let mut value: Value = serde_json::from_str(FIXTURE)?;
        value["properties"]["timeseries"][0]["data"]["instant"]["details"]["precipitation_rate"] =
            json!(1.5);
        value["properties"]["timeseries"][0]["data"]["next_1_hours"]["details"]
            ["precipitation_amount"] = json!(0.25);

        let met = parse(value)?;

        assert_eq!(met.precipitation_rate, 1.5);
        assert_eq!(met.precipitation_amount, 0.25);
        Ok(())
    }

    #[test]
    fn rejects_short_coordinates_instead_of_panicking() -> Result<()> {
        let mut value: Value = serde_json::from_str(FIXTURE)?;
        value["geometry"]["coordinates"] = json!([10.4]);

        assert!(parse(value).is_err());
        Ok(())
    }

    #[test]
    fn rejects_missing_values() -> Result<()> {
        let mut value: Value = serde_json::from_str(FIXTURE)?;
        value["properties"]["timeseries"][0]["data"]["instant"]["details"]
            .as_object_mut()
            .unwrap()
            .remove("wind_speed");

        assert!(parse(value).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn fetches_nowcast() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/nowcast")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("lat".into(), "63.4308".into()),
                mockito::Matcher::UrlEncoded("lon".into(), "10.4034".into()),
            ]))
            .with_status(200)
            .with_body(FIXTURE)
            .create_async()
            .await;

        let nowcast = MetNowcast::fetch(
            &Client::new(),
            &format!("{}/nowcast", server.url()),
            &Coordinates::new(10.4034, 63.4308)?,
        )
        .await?;

        mock.assert_async().await;
        assert!(matches!(nowcast, Nowcast::Met(_)));
        Ok(())
    }
}
