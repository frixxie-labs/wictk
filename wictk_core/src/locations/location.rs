use std::collections::HashMap;

use anyhow::{Context, Result};
use redact::Secret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::Coordinates;

pub const OPENWEATHERMAP_GEOCODING_URL: &str = "https://api.openweathermap.org/geo/1.0/direct";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct OpenWeatherMapLocation {
    pub name: String,
    pub local_names: Option<HashMap<String, String>>,
    #[serde(flatten)]
    pub location: Coordinates,
    pub country: String,
    pub state: Option<String>,
}

impl OpenWeatherMapLocation {
    pub async fn fetch(
        client: &Client,
        url: &str,
        location: &str,
        apikey: &Secret<String>,
    ) -> Result<Vec<Self>> {
        client
            .get(url)
            .query(&[("q", location), ("appid", apikey.expose_secret())])
            .send()
            .await
            .context("Failed to fetch location data from OpenWeatherMap")?
            .error_for_status()
            .context("OpenWeatherMap geocoding returned an error")?
            .json::<Vec<Self>>()
            .await
            .context("Failed to decode location data from OpenWeatherMap")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocationError {
    UnparsableLongitude(String),
    UnparsableLatitude(String),
    LongitudeOutOfRange(f32),
    LatitudeOutOfRange(f32),
}

impl std::fmt::Display for LocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::UnparsableLongitude(lon) => write!(f, "Could not parse longitude: {lon}"),
            Self::UnparsableLatitude(lat) => write!(f, "Could not parse latitude: {lat}"),
            Self::LongitudeOutOfRange(lon) => {
                write!(f, "Longitude {lon} is outside the range [-180, 180]")
            }
            Self::LatitudeOutOfRange(lat) => {
                write!(f, "Latitude {lat} is outside the range [-90, 90]")
            }
        }
    }
}

impl std::error::Error for LocationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetches_and_decodes_locations() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/geo")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("q".into(), "Oslo".into()),
                mockito::Matcher::UrlEncoded("appid".into(), "key".into()),
            ]))
            .with_status(200)
            .with_body(r#"[{"name":"Oslo","lat":59.91,"lon":10.75,"country":"NO"}]"#)
            .create_async()
            .await;

        let locations = OpenWeatherMapLocation::fetch(
            &Client::new(),
            &format!("{}/geo", server.url()),
            "Oslo",
            &Secret::new("key".to_string()),
        )
        .await?;

        mock.assert_async().await;
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].location, Coordinates::new(10.75, 59.91)?);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unsuccessful_response() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/geo")
            .match_query(mockito::Matcher::Any)
            .with_status(401)
            .create_async()
            .await;

        let result = OpenWeatherMapLocation::fetch(
            &Client::new(),
            &format!("{}/geo", server.url()),
            "Oslo",
            &Secret::new("key".to_string()),
        )
        .await;

        assert!(result.is_err());
    }
}
