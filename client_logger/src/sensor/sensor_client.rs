use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};

use super::types::{Sensor, SensorIds};
use super::SensorApi;

pub struct SensorClient {
    client: reqwest::Client,
    cache: BTreeMap<String, Sensor>,
}

impl SensorClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: BTreeMap::new(),
        }
    }

    pub fn lookup_sensor(&self, name: &str) -> Option<&Sensor> {
        self.cache.get(name)
    }
}

impl SensorApi for SensorClient {
    async fn get_sensors(&mut self, url: &str) -> Result<Vec<Sensor>> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to fetch sensors")?;

        let sensors = response
            .json::<Vec<Sensor>>()
            .await
            .context("Failed to parse sensors response")?;

        for sensor in &sensors {
            self.cache.insert(sensor.name.clone(), sensor.clone());
        }

        Ok(sensors)
    }

    async fn setup_sensor(
        &mut self,
        url: &str,
        sensor_name: &str,
        sensor_unit: &str,
    ) -> Result<i32> {
        if let Some(cached) = self.lookup_sensor(sensor_name) {
            tracing::info!("Found cached sensor: {:?}", cached);
            return Ok(cached.id);
        }

        let sensors = self.get_sensors(url).await?;
        let sensor = sensors.iter().find(|s| s.name == sensor_name);

        match sensor {
            Some(s) => {
                tracing::info!("Found existing sensor: {:?}", s);
                Ok(s.id)
            }
            None => {
                let new_sensor = Sensor {
                    id: 0,
                    name: sensor_name.to_string(),
                    unit: sensor_unit.to_string(),
                };

                let response = self
                    .client
                    .post(url)
                    .json(&new_sensor)
                    .send()
                    .await
                    .context("Failed to create sensor")?;

                tracing::info!("Created new sensor: {:?}", response);

                let sensors = self.get_sensors(url).await?;
                sensors
                    .iter()
                    .find(|s| s.name == sensor_name)
                    .map(|s| s.id)
                    .ok_or_else(|| anyhow!("Sensor not found after creation"))
            }
        }
    }

    async fn setup_sensors(&mut self, url: &str) -> Result<SensorIds> {
        let temperature = self.setup_sensor(url, "temperature", "°C").await?;
        let humidity = self.setup_sensor(url, "humidity", "%").await?;
        let wind_speed = self.setup_sensor(url, "wind_speed", "m/s").await?;
        let wind_deg = self.setup_sensor(url, "wind_deg", "°").await?;
        let precipitation_rate = self.setup_sensor(url, "precipitation_rate", "mm/h").await?;
        let precipitation_amount = self.setup_sensor(url, "precipitation_amount", "mm").await?;
        let wind_speed_gust = self.setup_sensor(url, "wind_speed_gust", "m/s").await?;
        let feels_like = self.setup_sensor(url, "feels_like", "°C").await?;
        let pressure = self.setup_sensor(url, "pressure", "hPa").await?;
        let clouds = self.setup_sensor(url, "clouds", "%").await?;
        let visibility = self.setup_sensor(url, "visibility", "m").await?;
        let lon = self.setup_sensor(url, "lon", "°").await?;
        let lat = self.setup_sensor(url, "lat", "°").await?;
        let alert_count = self.setup_sensor(url, "alert_count", "count").await?;
        let earthquake_depth = self.setup_sensor(url, "earthquake_depth", "km").await?;
        let earthquake_magnitude = self
            .setup_sensor(url, "earthquake_magnitude", "magnitude")
            .await?;
        let earthquake_significance = self
            .setup_sensor(url, "earthquake_significance", "score")
            .await?;
        let earthquake_tsunami = self
            .setup_sensor(url, "earthquake_tsunami", "boolean")
            .await?;

        Ok(SensorIds {
            temperature,
            humidity,
            wind_speed,
            wind_deg,
            precipitation_rate,
            precipitation_amount,
            wind_speed_gust,
            feels_like,
            pressure,
            clouds,
            visibility,
            lon,
            lat,
            alert_count,
            earthquake_depth,
            earthquake_magnitude,
            earthquake_significance,
            earthquake_tsunami,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_client() -> SensorClient {
        SensorClient::new(reqwest::Client::new())
    }

    #[tokio::test]
    async fn should_fetch_sensors_successfully() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {"id": 1, "name": "temperature", "unit": "°C"},
                {"id": 2, "name": "humidity", "unit": "%"}
            ]"#,
            )
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client.get_sensors(&server.url()).await;

        assert!(result.is_ok());
        let sensors = result.unwrap();
        assert_eq!(sensors.len(), 2);
        assert_eq!(sensors[0].id, 1);
        assert_eq!(sensors[0].name, "temperature");
        assert_eq!(sensors[0].unit, "°C");
        assert_eq!(sensors[1].id, 2);
        assert_eq!(sensors[1].name, "humidity");
        assert_eq!(sensors[1].unit, "%");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_setup_existing_sensor() {
        let mut server = mockito::Server::new_async().await;
        let mock_get = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {"id": 1, "name": "temperature", "unit": "°C"}
            ]"#,
            )
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client
            .setup_sensor(&server.url(), "temperature", "°C")
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
        mock_get.assert_async().await;
    }

    #[tokio::test]
    async fn should_setup_new_sensor() {
        let mut server = mockito::Server::new_async().await;

        let mock_get_empty = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let mock_post = server
            .mock("POST", "/")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id": 2, "name": "temperature", "unit": "°C"}"#)
            .create_async()
            .await;

        let mock_get_created = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {"id": 2, "name": "temperature", "unit": "°C"}
            ]"#,
            )
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client
            .setup_sensor(&server.url(), "temperature", "°C")
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);

        mock_get_empty.assert_async().await;
        mock_post.assert_async().await;
        mock_get_created.assert_async().await;
    }

    #[tokio::test]
    async fn should_return_error_when_sensor_creation_fails() {
        let mut server = mockito::Server::new_async().await;

        let mock_get_empty = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let mock_post = server
            .mock("POST", "/")
            .with_status(201)
            .create_async()
            .await;

        let mock_get_still_empty = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client
            .setup_sensor(&server.url(), "ghost_sensor", "unit")
            .await;

        assert!(result.is_err());

        mock_get_empty.assert_async().await;
        mock_post.assert_async().await;
        mock_get_still_empty.assert_async().await;
    }

    #[tokio::test]
    async fn should_return_error_when_get_sensors_returns_invalid_json() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json")
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client.get_sensors(&server.url()).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to parse sensors response"),
            "Expected parse error, got: {err_msg}"
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_propagate_get_sensors_error_in_setup_sensor() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json")
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client
            .setup_sensor(&server.url(), "any_sensor", "any_unit")
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to parse sensors response"),
            "Expected parse error propagated from get_sensors, got: {err_msg}"
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_setup_sensors_successfully() {
        let mut server = mockito::Server::new_async().await;

        let mock_get = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {"id": 1, "name": "temperature", "unit": "°C"},
                {"id": 2, "name": "humidity", "unit": "%"},
                {"id": 3, "name": "wind_speed", "unit": "m/s"},
                {"id": 4, "name": "wind_deg", "unit": "°"},
                {"id": 5, "name": "precipitation_rate", "unit": "mm/h"},
                {"id": 6, "name": "precipitation_amount", "unit": "mm"},
                {"id": 7, "name": "wind_speed_gust", "unit": "m/s"},
                {"id": 8, "name": "feels_like", "unit": "°C"},
                {"id": 9, "name": "pressure", "unit": "hPa"},
                {"id": 10, "name": "clouds", "unit": "%"},
                {"id": 11, "name": "visibility", "unit": "m"},
                {"id": 12, "name": "lon", "unit": "°"},
                {"id": 13, "name": "lat", "unit": "°"},
                {"id": 14, "name": "alert_count", "unit": "count"},
                {"id": 15, "name": "earthquake_depth", "unit": "km"},
                {"id": 16, "name": "earthquake_magnitude", "unit": "magnitude"},
                {"id": 17, "name": "earthquake_significance", "unit": "score"},
                {"id": 18, "name": "earthquake_tsunami", "unit": "boolean"}
            ]"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let result = sensor_client.setup_sensors(&server.url()).await;

        assert!(result.is_ok());
        let sensor_ids = result.unwrap();
        assert_eq!(sensor_ids.temperature, 1);
        assert_eq!(sensor_ids.humidity, 2);
        assert_eq!(sensor_ids.wind_speed, 3);
        assert_eq!(sensor_ids.wind_deg, 4);
        assert_eq!(sensor_ids.precipitation_rate, 5);
        assert_eq!(sensor_ids.precipitation_amount, 6);
        assert_eq!(sensor_ids.wind_speed_gust, 7);
        assert_eq!(sensor_ids.feels_like, 8);
        assert_eq!(sensor_ids.pressure, 9);
        assert_eq!(sensor_ids.clouds, 10);
        assert_eq!(sensor_ids.visibility, 11);
        assert_eq!(sensor_ids.lon, 12);
        assert_eq!(sensor_ids.lat, 13);
        assert_eq!(sensor_ids.alert_count, 14);
        assert_eq!(sensor_ids.earthquake_depth, 15);
        assert_eq!(sensor_ids.earthquake_magnitude, 16);
        assert_eq!(sensor_ids.earthquake_significance, 17);
        assert_eq!(sensor_ids.earthquake_tsunami, 18);

        mock_get.assert_async().await;
    }

    #[tokio::test]
    async fn should_populate_cache_after_get_sensors() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                {"id": 1, "name": "temperature", "unit": "°C"},
                {"id": 2, "name": "humidity", "unit": "%"}
            ]"#,
            )
            .create_async()
            .await;

        let mut sensor_client = make_client();
        assert!(sensor_client.lookup_sensor("temperature").is_none());

        sensor_client.get_sensors(&server.url()).await.unwrap();

        let cached = sensor_client.lookup_sensor("temperature");
        assert!(cached.is_some());
        let sensor = cached.unwrap();
        assert_eq!(sensor.id, 1);
        assert_eq!(sensor.name, "temperature");
        assert_eq!(sensor.unit, "°C");

        let cached = sensor_client.lookup_sensor("humidity");
        assert!(cached.is_some());
        let sensor = cached.unwrap();
        assert_eq!(sensor.id, 2);
        assert_eq!(sensor.name, "humidity");
        assert_eq!(sensor.unit, "%");

        assert!(sensor_client.lookup_sensor("nonexistent").is_none());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_update_cache_on_subsequent_get_sensors() {
        let mut server = mockito::Server::new_async().await;

        let mock_first = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id": 1, "name": "temperature", "unit": "°C"}]"#)
            .create_async()
            .await;

        let mock_second = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id": 1, "name": "temperature", "unit": "°F"}]"#)
            .create_async()
            .await;

        let mut sensor_client = make_client();

        sensor_client.get_sensors(&server.url()).await.unwrap();
        assert_eq!(
            sensor_client.lookup_sensor("temperature").unwrap().unit,
            "°C"
        );

        sensor_client.get_sensors(&server.url()).await.unwrap();
        assert_eq!(
            sensor_client.lookup_sensor("temperature").unwrap().unit,
            "°F"
        );

        mock_first.assert_async().await;
        mock_second.assert_async().await;
    }

    #[tokio::test]
    async fn should_not_populate_cache_on_failed_get_sensors() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json")
            .create_async()
            .await;

        let mut sensor_client = make_client();
        let _ = sensor_client.get_sensors(&server.url()).await;

        assert!(sensor_client.lookup_sensor("temperature").is_none());

        mock.assert_async().await;
    }
}
