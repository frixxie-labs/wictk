use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::device::DeviceId;
use crate::measurement::NewMeasurement;
use crate::sensor::SensorIds;

use super::StorageApi;

pub struct StorageClient {
    client: reqwest::Client,
}

impl StorageClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl StorageApi for StorageClient {
    async fn store_met_nowcast(
        &self,
        url: &str,
        met_nowcast: &wictk_core::MetNowcast,
        device_id: &DeviceId,
        sensor_ids: &SensorIds,
    ) -> Result<()> {
        tracing::info!("Storing MET nowcast with timestamp: {}", met_nowcast.time);
        let temperature = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.temperature,
            met_nowcast.air_temperature,
        );
        let humidity = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.humidity,
            met_nowcast.relative_humidity,
        );
        let wind_speed = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.wind_speed,
            met_nowcast.wind_speed,
        );
        let wind_deg = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.wind_deg,
            met_nowcast.wind_from_direction,
        );
        let precipitation_rate = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.precipitation_rate,
            met_nowcast.precipitation_rate,
        );
        let precipitation_amount = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.precipitation_amount,
            met_nowcast.precipitation_amount,
        );
        let wind_speed_gust = NewMeasurement::new_with_ts(
            met_nowcast.time,
            *device_id,
            sensor_ids.wind_speed_gust,
            met_nowcast.wind_speed_gust,
        );

        self.client
            .post(url)
            .json(&vec![
                &temperature,
                &humidity,
                &wind_speed,
                &wind_deg,
                &precipitation_rate,
                &precipitation_amount,
                &wind_speed_gust,
            ])
            .send()
            .await
            .with_context(|| format!("Failed to send MET nowcast measurements to {url}"))?
            .error_for_status()
            .with_context(|| {
                format!("Storage service rejected MET nowcast measurements at {url}")
            })?;

        Ok(())
    }

    async fn store_openweather_nowcast(
        &self,
        url: &str,
        open_weather_nowcast: &wictk_core::OpenWeatherNowcast,
        device_id: &DeviceId,
        sensor_ids: &SensorIds,
    ) -> Result<()> {
        tracing::info!(
            "Storing OpenWeather nowcast with timestamp: {}",
            open_weather_nowcast.dt
        );
        let temperature = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.temperature,
            open_weather_nowcast.temp,
        );
        let humidity = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.humidity,
            open_weather_nowcast.humidity as f32,
        );
        let wind_speed = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.wind_speed,
            open_weather_nowcast.wind_speed,
        );
        let wind_deg = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.wind_deg,
            open_weather_nowcast.wind_deg as f32,
        );
        let feels_like = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.feels_like,
            open_weather_nowcast.feels_like,
        );
        let pressure = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.pressure,
            open_weather_nowcast.pressure as f32,
        );
        let clouds = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.clouds,
            open_weather_nowcast.clouds as f32,
        );
        let visibility = NewMeasurement::new_with_ts(
            open_weather_nowcast.dt,
            *device_id,
            sensor_ids.visibility,
            open_weather_nowcast.visibility as f32,
        );

        self.client
            .post(url)
            .json(&vec![
                &temperature,
                &humidity,
                &wind_speed,
                &wind_deg,
                &feels_like,
                &pressure,
                &clouds,
                &visibility,
            ])
            .send()
            .await
            .with_context(|| format!("Failed to send OpenWeather nowcast measurements to {url}"))?
            .error_for_status()
            .with_context(|| {
                format!("Storage service rejected OpenWeather nowcast measurements at {url}")
            })?;

        Ok(())
    }

    async fn store_lightnings(
        &self,
        url: &str,
        device_id: &DeviceId,
        lon_id: i32,
        lat_id: i32,
        lightnings: &[wictk_core::Lightning],
    ) -> Result<()> {
        tracing::info!(
            "Storing batch of {} lightning measurements",
            lightnings.len()
        );
        let measurements: Vec<NewMeasurement> = lightnings
            .iter()
            .flat_map(|lightning| {
                [
                    NewMeasurement::new_with_ts(
                        lightning.time,
                        *device_id,
                        lat_id,
                        lightning.location.y() as f32,
                    ),
                    NewMeasurement::new_with_ts(
                        lightning.time,
                        *device_id,
                        lon_id,
                        lightning.location.x() as f32,
                    ),
                ]
            })
            .collect();

        self.client
            .post(url)
            .json(&measurements)
            .send()
            .await
            .with_context(|| format!("Failed to send lightning batch to {url}"))?
            .error_for_status()
            .with_context(|| format!("Storage service rejected lightning batch at {url}"))?;

        Ok(())
    }

    async fn store_alert_count(
        &self,
        url: &str,
        timestamp: DateTime<Utc>,
        device_id: &DeviceId,
        alert_count_sensor_id: i32,
        alert_count: usize,
    ) -> Result<()> {
        tracing::info!("Storing alert count measurement: {}", alert_count);

        let alert_count = NewMeasurement::new_with_ts(
            timestamp,
            *device_id,
            alert_count_sensor_id,
            alert_count as f32,
        );

        self.client
            .post(url)
            .json(&vec![&alert_count])
            .send()
            .await
            .with_context(|| format!("Failed to send alert count measurement to {url}"))?
            .error_for_status()
            .with_context(|| {
                format!("Storage service rejected alert count measurement at {url}")
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use geo::Point;

    fn make_client() -> StorageClient {
        StorageClient::new(reqwest::Client::new())
    }

    fn make_sensor_ids() -> SensorIds {
        SensorIds {
            temperature: 1,
            humidity: 2,
            wind_speed: 3,
            wind_deg: 4,
            precipitation_rate: 5,
            precipitation_amount: 6,
            wind_speed_gust: 7,
            feels_like: 8,
            pressure: 9,
            clouds: 10,
            visibility: 11,
            lon: 12,
            lat: 13,
            alert_count: 14,
        }
    }

    fn make_timestamp() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2025-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn should_store_met_nowcast() {
        let mut server = mockito::Server::new_async().await;
        let mock = server.mock("POST", "/")
            .with_status(200)
            .match_body(mockito::Matcher::JsonString(
                r#"[{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":1,"measurement":20.5},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":2,"measurement":65.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":3,"measurement":5.2},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":4,"measurement":180.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":5,"measurement":0.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":6,"measurement":0.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":7,"measurement":6.0}]"#.to_string()
            ))
            .create_async()
            .await;

        let met_nowcast = wictk_core::MetNowcast {
            time: make_timestamp(),
            location: wictk_core::Coordinates {
                lon: 10.0,
                lat: 63.0,
            },
            description: "Clear".to_string(),
            air_temperature: 20.5,
            relative_humidity: 65.0,
            precipitation_rate: 0.0,
            precipitation_amount: 0.0,
            wind_speed: 5.2,
            wind_speed_gust: 6.0,
            wind_from_direction: 180.0,
        };

        let storage_client = make_client();
        let result = storage_client
            .store_met_nowcast(&server.url(), &met_nowcast, &1, &make_sensor_ids())
            .await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_store_openweather_nowcast() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .create_async()
            .await;

        let openweather_nowcast = wictk_core::OpenWeatherNowcast {
            dt: make_timestamp(),
            name: "Trondheim".to_string(),
            country: "NO".to_string(),
            lon: 10.0,
            lat: 63.0,
            main: "Clear".to_string(),
            desc: "clear sky".to_string(),
            clouds: 0,
            wind_speed: 4.1,
            wind_deg: 200,
            visibility: 10000,
            temp: 22.3,
            feels_like: 23.0,
            humidity: 70,
            pressure: 1013,
        };

        let storage_client = make_client();
        let result = storage_client
            .store_openweather_nowcast(&server.url(), &openweather_nowcast, &1, &make_sensor_ids())
            .await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_store_met_nowcast_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        let met_nowcast = wictk_core::MetNowcast {
            time: make_timestamp(),
            location: wictk_core::Coordinates {
                lon: 10.0,
                lat: 63.0,
            },
            description: "Clear".to_string(),
            air_temperature: 20.5,
            relative_humidity: 65.0,
            precipitation_rate: 0.0,
            precipitation_amount: 0.0,
            wind_speed: 5.2,
            wind_speed_gust: 6.0,
            wind_from_direction: 180.0,
        };

        let storage_client = make_client();
        let result = storage_client
            .store_met_nowcast(&server.url(), &met_nowcast, &1, &make_sensor_ids())
            .await;

        assert!(
            result.is_err(),
            "Expected error for HTTP 500 but got: {result:?}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_store_openweather_nowcast_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        let openweather_nowcast = wictk_core::OpenWeatherNowcast {
            dt: make_timestamp(),
            name: "Trondheim".to_string(),
            country: "NO".to_string(),
            lon: 10.0,
            lat: 63.0,
            main: "Clear".to_string(),
            desc: "clear sky".to_string(),
            clouds: 0,
            wind_speed: 4.1,
            wind_deg: 200,
            visibility: 10000,
            temp: 22.3,
            feels_like: 23.0,
            humidity: 70,
            pressure: 1013,
        };

        let storage_client = make_client();
        let result = storage_client
            .store_openweather_nowcast(&server.url(), &openweather_nowcast, &1, &make_sensor_ids())
            .await;

        assert!(
            result.is_err(),
            "Expected error for HTTP 500 but got: {result:?}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_store_lightnings() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .match_body(mockito::Matcher::JsonString(
                r#"[{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":13,"measurement":63.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":12,"measurement":10.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":13,"measurement":64.0},{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":12,"measurement":11.0}]"#.to_string()
            ))
            .create_async()
            .await;

        let lightnings = vec![
            wictk_core::Lightning {
                time: make_timestamp(),
                location: Point::new(10.0, 63.0),
                magic_value: 42,
            },
            wictk_core::Lightning {
                time: make_timestamp(),
                location: Point::new(11.0, 64.0),
                magic_value: 43,
            },
        ];

        let storage_client = make_client();
        let result = storage_client
            .store_lightnings(&server.url(), &1, 12, 13, &lightnings)
            .await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_store_alert_count() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .match_body(mockito::Matcher::JsonString(
                r#"[{"timestamp":"2025-08-11T12:00:00Z","device":1,"sensor":14,"measurement":2.0}]"#
                    .to_string(),
            ))
            .create_async()
            .await;

        let storage_client = make_client();
        let result = storage_client
            .store_alert_count(&server.url(), make_timestamp(), &1, 14, 2)
            .await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_store_alert_count_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        let storage_client = make_client();
        let result = storage_client
            .store_alert_count(&server.url(), make_timestamp(), &1, 14, 2)
            .await;

        assert!(
            result.is_err(),
            "Expected error for HTTP 500 but got: {result:?}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_handle_store_lightnings_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        let lightnings = vec![wictk_core::Lightning {
            time: make_timestamp(),
            location: Point::new(10.0, 63.0),
            magic_value: 42,
        }];

        let storage_client = make_client();
        let result = storage_client
            .store_lightnings(&server.url(), &1, 12, 13, &lightnings)
            .await;

        assert!(
            result.is_err(),
            "Expected error for HTTP 500 but got: {result:?}"
        );
        mock.assert_async().await;
    }
}
