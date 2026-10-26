use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use redact::Secret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::locations::Coordinates;

use super::Nowcast;

pub const OPENWEATHERMAP_NOWCAST_URL: &str = "https://api.openweathermap.org/data/2.5/weather";

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct OpenWeatherNowcast {
    pub dt: DateTime<Utc>,
    pub name: String,
    pub country: String,
    pub lon: f32,
    pub lat: f32,
    pub main: String,
    pub desc: String,
    pub clouds: u32,
    pub wind_speed: f32,
    pub wind_deg: i32,
    pub visibility: i32,
    pub temp: f32,
    pub feels_like: f32,
    pub humidity: u32,
    pub pressure: u32,
}

impl std::fmt::Display for OpenWeatherNowcast {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}",
            self.dt,
            self.name,
            self.country,
            self.lon,
            self.lat,
            self.main,
            self.desc,
            self.clouds,
            self.wind_speed,
            self.wind_deg,
            self.visibility,
            self.temp,
            self.feels_like,
            self.humidity,
            self.pressure
        )
    }
}

#[derive(Deserialize)]
struct OwmResponse {
    #[serde(with = "chrono::serde::ts_seconds")]
    dt: DateTime<Utc>,
    name: String,
    sys: OwmSys,
    coord: OwmCoord,
    weather: Vec<OwmWeather>,
    clouds: OwmClouds,
    wind: OwmWind,
    visibility: i32,
    main: OwmMain,
}

#[derive(Deserialize)]
struct OwmSys {
    country: String,
}

#[derive(Deserialize)]
struct OwmCoord {
    lon: f32,
    lat: f32,
}

#[derive(Deserialize)]
struct OwmWeather {
    main: String,
    description: String,
}

#[derive(Deserialize)]
struct OwmClouds {
    all: u32,
}

#[derive(Deserialize)]
struct OwmWind {
    speed: f32,
    deg: i32,
}

#[derive(Deserialize)]
struct OwmMain {
    temp: f32,
    feels_like: f32,
    humidity: u32,
    pressure: u32,
}

impl TryFrom<OwmResponse> for OpenWeatherNowcast {
    type Error = anyhow::Error;

    fn try_from(response: OwmResponse) -> Result<Self> {
        let weather = response
            .weather
            .into_iter()
            .next()
            .context("OpenWeatherMap response contains no weather description")?;

        Ok(Self {
            dt: response.dt,
            name: response.name,
            country: response.sys.country,
            lon: response.coord.lon,
            lat: response.coord.lat,
            main: weather.main,
            desc: weather.description,
            clouds: response.clouds.all,
            wind_speed: response.wind.speed,
            wind_deg: response.wind.deg,
            visibility: response.visibility,
            temp: response.main.temp,
            feels_like: response.main.feels_like,
            humidity: response.main.humidity,
            pressure: response.main.pressure,
        })
    }
}

impl From<OpenWeatherNowcast> for Nowcast {
    fn from(open_weather: OpenWeatherNowcast) -> Self {
        Self::OpenWeather(open_weather)
    }
}

impl OpenWeatherNowcast {
    pub async fn fetch(
        client: &Client,
        url: &str,
        location: &Coordinates,
        apikey: &Secret<String>,
    ) -> Result<Nowcast> {
        let response = client
            .get(url)
            .query(&[("lat", location.lat()), ("lon", location.lon())])
            .query(&[
                ("appid", apikey.expose_secret().as_str()),
                ("units", "metric"),
            ])
            .send()
            .await
            .context("Request to OpenWeatherMap failed")?
            .error_for_status()
            .context("OpenWeatherMap nowcast returned an error")?
            .json::<OwmResponse>()
            .await
            .context("Deserialization from OpenWeatherMap failed")?;

        Ok(OpenWeatherNowcast::try_from(response)?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const FIXTURE: &str = include_str!("../../tests/fixtures/openweathermap_nowcast.json");

    fn parse(value: Value) -> Result<OpenWeatherNowcast> {
        OpenWeatherNowcast::try_from(serde_json::from_value::<OwmResponse>(value)?)
    }

    #[test]
    fn parses_nowcast() -> Result<()> {
        let open_weather = parse(serde_json::from_str(FIXTURE)?)?;

        assert_eq!(open_weather.dt.timestamp(), 1692185222);
        assert_eq!(open_weather.name, "Trondheim");
        assert_eq!(open_weather.country, "NO");
        assert_eq!(open_weather.lon, 10.3951);
        assert_eq!(open_weather.lat, 63.4305);
        assert_eq!(open_weather.main, "Clouds");
        assert_eq!(open_weather.desc, "overcast clouds");
        assert_eq!(open_weather.clouds, 99);
        assert_eq!(open_weather.wind_speed, 0.89);
        assert_eq!(open_weather.wind_deg, 270);
        assert_eq!(open_weather.visibility, 10000);
        assert_eq!(open_weather.temp, 15.21);
        assert_eq!(open_weather.feels_like, 15.19);
        assert_eq!(open_weather.humidity, 92);
        assert_eq!(open_weather.pressure, 1014);
        Ok(())
    }

    #[test]
    fn rejects_empty_weather_list() -> Result<()> {
        let mut value: Value = serde_json::from_str(FIXTURE)?;
        value["weather"] = json!([]);

        assert!(parse(value).is_err());
        Ok(())
    }

    #[test]
    fn rejects_missing_fields() -> Result<()> {
        let mut value: Value = serde_json::from_str(FIXTURE)?;
        value.as_object_mut().unwrap().remove("main");

        assert!(parse(value).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unsuccessful_response() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/weather")
            .match_query(mockito::Matcher::Any)
            .with_status(401)
            .create_async()
            .await;

        let result = OpenWeatherNowcast::fetch(
            &Client::new(),
            &format!("{}/weather", server.url()),
            &Coordinates::new(10.0, 60.0)?,
            &Secret::new("key".to_string()),
        )
        .await;

        assert!(result.is_err());
        Ok(())
    }
}
