use anyhow::{bail, Context, Result};
use tracing::instrument;

use super::WeatherApi;

pub struct WeatherClient {
    client: reqwest::Client,
}

impl WeatherClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl WeatherApi for WeatherClient {
    #[instrument(skip(self), fields(url = %url, location = %location))]
    async fn get_nowcast(&self, url: &str, location: &str) -> Result<Vec<wictk_core::Nowcast>> {
        tracing::debug!("Fetching nowcast data");
        let full_url = format!("{url}api/nowcasts?location={location}");
        tracing::info!("Requesting nowcast data from: {}", full_url);

        let response = self
            .client
            .get(&full_url)
            .send()
            .await
            .context("Failed to fetch nowcast data")?;

        tracing::debug!("Response status: {}", response.status());

        if response.status().is_success() {
            let nowcasts: Vec<wictk_core::Nowcast> = response
                .json()
                .await
                .context("Failed to parse nowcast response")?;
            tracing::info!("Successfully fetched {} nowcast records", nowcasts.len());
            Ok(nowcasts)
        } else {
            tracing::error!("Failed to fetch nowcast data: HTTP {}", response.status());
            bail!("HTTP error: {}", response.status())
        }
    }

    #[instrument(skip(self), fields(url = %url))]
    async fn get_lightnings(&self, url: &str) -> Result<Vec<wictk_core::Lightning>> {
        tracing::debug!("Fetching lightning data");
        let full_url = format!("{url}api/recent_lightning");
        tracing::info!("Requesting lightning data from: {}", full_url);

        let response = self
            .client
            .get(&full_url)
            .send()
            .await
            .context("Failed to fetch lightning data")?;

        tracing::debug!("Response status: {}", response.status());

        if response.status().is_success() {
            let lightnings: Vec<wictk_core::Lightning> = response
                .json()
                .await
                .context("Failed to parse lightning response")?;
            tracing::info!(
                "Successfully fetched {} lightning records",
                lightnings.len()
            );
            Ok(lightnings)
        } else {
            tracing::error!("Failed to fetch lightning data: HTTP {}", response.status());
            bail!("HTTP error: {}", response.status())
        }
    }

    #[instrument(skip(self), fields(url = %url))]
    async fn get_alerts(&self, url: &str) -> Result<Vec<wictk_core::Alert>> {
        tracing::debug!("Fetching alert data");
        let full_url = format!("{url}api/alerts");
        tracing::info!("Requesting alert data from: {}", full_url);

        let response = self
            .client
            .get(&full_url)
            .send()
            .await
            .context("Failed to fetch alert data")?;

        tracing::debug!("Response status: {}", response.status());

        if response.status().is_success() {
            let alerts: Vec<wictk_core::Alert> = response
                .json()
                .await
                .context("Failed to parse alert response")?;
            tracing::info!("Successfully fetched {} alert records", alerts.len());
            Ok(alerts)
        } else {
            tracing::error!("Failed to fetch alert data: HTTP {}", response.status());
            bail!("HTTP error: {}", response.status())
        }
    }

    #[instrument(skip(self), fields(url = %url, lat, lon, radius_km))]
    async fn get_earthquakes(
        &self,
        url: &str,
        lat: f64,
        lon: f64,
        radius_km: f64,
    ) -> Result<Vec<wictk_core::Earthquake>> {
        let full_url = format!("{url}api/earthquakes");
        tracing::info!("Checking for earthquakes near {}, {}", lat, lon);

        let response = self
            .client
            .get(&full_url)
            .query(&[
                ("lat", &lat.to_string()),
                ("lon", &lon.to_string()),
                ("radius_km", &radius_km.to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch earthquake data")?
            .error_for_status()
            .context("Failed to fetch earthquake data")?;

        let earthquakes = response
            .json::<Vec<wictk_core::Earthquake>>()
            .await
            .context("Failed to parse earthquake response")?;
        tracing::info!(
            "Successfully fetched {} earthquake records near {}, {}",
            earthquakes.len(),
            lat,
            lon
        );
        Ok(earthquakes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wictk_core::Nowcast;

    fn make_client() -> WeatherClient {
        WeatherClient::new(reqwest::Client::new())
    }

    #[tokio::test]
    async fn should_get_nowcast_successfully() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(mockito::Matcher::UrlEncoded(
                "location".into(),
                "Trondheim".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "met": {
                        "time": "2025-08-11T12:00:00Z",
                        "location": {"lon": 10.0, "lat": 63.0},
                        "description": "Clear",
                        "air_temperature": 20.5,
                        "relative_humidity": 65.0,
                        "precipitation_rate": 0.0,
                        "precipitation_amount": 0.0,
                        "wind_speed": 5.2,
                        "wind_speed_gust": 6.0,
                        "wind_from_direction": 180.0
                    }
                }
            ]"#,
            )
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_nowcast(&format!("{}/", server.url()), "Trondheim")
            .await;

        assert!(result.is_ok());
        let nowcasts = result.unwrap();
        assert_eq!(nowcasts.len(), 1);

        match &nowcasts[0] {
            Nowcast::Met(met) => {
                assert_eq!(met.air_temperature, 20.5);
                assert_eq!(met.relative_humidity, 65.0);
                assert_eq!(met.wind_speed, 5.2);
                assert_eq!(met.wind_from_direction, 180.0);
            }
            _ => panic!("Expected Met nowcast"),
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_empty_nowcast_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(mockito::Matcher::UrlEncoded(
                "location".into(),
                "TestLocation".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_nowcast(&format!("{}/", server.url()), "TestLocation")
            .await;

        assert!(result.is_ok());
        let nowcasts = result.unwrap();
        assert_eq!(nowcasts.len(), 0);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_nowcast_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(mockito::Matcher::UrlEncoded(
                "location".into(),
                "ErrorLocation".into(),
            ))
            .with_status(500)
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_nowcast(&format!("{}/", server.url()), "ErrorLocation")
            .await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_lightnings_successfully() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/recent_lightning")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "time": "2025-08-11T12:00:00Z",
                    "location": {
                        "x": 10.0,
                        "y": 63.0
                    },
                    "magic_value": 42
                }
            ]"#,
            )
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_lightnings(&format!("{}/", server.url()))
            .await;

        assert!(result.is_ok());
        let lightnings = result.unwrap();
        assert_eq!(lightnings.len(), 1);

        let lightning = &lightnings[0];
        assert_eq!(lightning.location.x(), 10.0);
        assert_eq!(lightning.location.y(), 63.0);
        assert_eq!(lightning.magic_value, 42);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_empty_lightnings_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/recent_lightning")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_lightnings(&format!("{}/", server.url()))
            .await;

        assert!(result.is_ok());
        let lightnings = result.unwrap();
        assert_eq!(lightnings.len(), 0);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_lightnings_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/recent_lightning")
            .with_status(500)
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_lightnings(&format!("{}/", server.url()))
            .await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_alerts_successfully() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/alerts")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"["Nve", "Nve"]"#)
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_alerts(&format!("{}/", server.url()))
            .await;

        assert!(result.is_ok());
        let alerts = result.unwrap();
        assert_eq!(alerts.len(), 2);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_alerts_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/alerts")
            .with_status(500)
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_alerts(&format!("{}/", server.url()))
            .await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_earthquakes_near_japan() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/earthquakes")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("lat".into(), "36.2048".into()),
                mockito::Matcher::UrlEncoded("lon".into(), "138.2529".into()),
                mockito::Matcher::UrlEncoded("radius_km".into(), "2000".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[{
                    "id": "us-test",
                    "magnitude": 4.7,
                    "place": "Near Japan",
                    "time": "2026-08-11T06:38:56Z",
                    "updated": "2026-08-11T06:42:10Z",
                    "coordinates": {"lon": 138.0, "lat": 36.0},
                    "depth_km": 8.3,
                    "significance": 340,
                    "alert_level": "green",
                    "status": "reviewed",
                    "tsunami": false,
                    "details_url": "https://example.com/us-test"
                }]"#,
            )
            .create_async()
            .await;

        let earthquakes = make_client()
            .get_earthquakes(&format!("{}/", server.url()), 36.2048, 138.2529, 2000.0)
            .await
            .unwrap();

        assert_eq!(earthquakes.len(), 1);
        assert_eq!(earthquakes[0].id, "us-test");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_nowcast_with_multiple_types() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(mockito::Matcher::UrlEncoded(
                "location".into(),
                "Mixed".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "met": {
                        "time": "2025-08-11T12:00:00Z",
                        "location": {"lon": 10.0, "lat": 63.0},
                        "description": "Clear",
                        "air_temperature": 20.5,
                        "relative_humidity": 65.0,
                        "precipitation_rate": 0.0,
                        "precipitation_amount": 0.0,
                        "wind_speed": 5.2,
                        "wind_speed_gust": 6.0,
                        "wind_from_direction": 180.0
                    }
                },
                {
                    "open_weather": {
                        "dt": "2025-08-11T13:00:00Z",
                        "name": "Mixed",
                        "country": "NO",
                        "lon": 10.0,
                        "lat": 63.0,
                        "main": "Clouds",
                        "desc": "few clouds",
                        "clouds": 20,
                        "wind_speed": 4.1,
                        "wind_deg": 200,
                        "visibility": 10000,
                        "temp": 22.3,
                        "feels_like": 23.0,
                        "humidity": 70,
                        "pressure": 1013
                    }
                }
            ]"#,
            )
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_nowcast(&format!("{}/", server.url()), "Mixed")
            .await;

        assert!(result.is_ok());
        let nowcasts = result.unwrap();
        assert_eq!(nowcasts.len(), 2);

        let has_met = nowcasts.iter().any(|n| matches!(n, Nowcast::Met(_)));
        let has_open_weather = nowcasts
            .iter()
            .any(|n| matches!(n, Nowcast::OpenWeather(_)));
        assert!(has_met);
        assert!(has_open_weather);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_lightnings_with_multiple_entries() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/recent_lightning")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {
                    "time": "2025-08-11T12:00:00Z",
                    "location": {
                        "x": 10.0,
                        "y": 63.0
                    },
                    "magic_value": 42
                },
                {
                    "time": "2025-08-11T12:05:00Z",
                    "location": {
                        "x": 11.0,
                        "y": 64.0
                    },
                    "magic_value": 24
                }
            ]"#,
            )
            .create_async()
            .await;

        let weather_client = make_client();
        let result = weather_client
            .get_lightnings(&format!("{}/", server.url()))
            .await;

        assert!(result.is_ok());
        let lightnings = result.unwrap();
        assert_eq!(lightnings.len(), 2);

        assert_eq!(lightnings[0].location.x(), 10.0);
        assert_eq!(lightnings[0].location.y(), 63.0);
        assert_eq!(lightnings[0].magic_value, 42);

        assert_eq!(lightnings[1].location.x(), 11.0);
        assert_eq!(lightnings[1].location.y(), 64.0);
        assert_eq!(lightnings[1].magic_value, 24);

        mock.assert_async().await;
    }
}
