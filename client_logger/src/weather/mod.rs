use anyhow::Result;
use reqwest::{Client, Url};
use wictk_core::{Alert, Coordinates, Earthquake, Lightning, Nowcast};

use crate::api::{endpoint, get_json};

/// Reads weather data from the WICTK backend.
pub struct WeatherClient {
    client: Client,
    base: Url,
}

impl WeatherClient {
    pub fn new(client: Client, base: Url) -> Self {
        Self { client, base }
    }

    pub async fn get_nowcasts(&self, location: &str) -> Result<Vec<Nowcast>> {
        let url = endpoint(&self.base, "api/nowcasts")?;
        get_json(&self.client, url, &[("location", location)]).await
    }

    pub async fn get_lightnings(&self) -> Result<Vec<Lightning>> {
        get_json(
            &self.client,
            endpoint(&self.base, "api/recent_lightning")?,
            &[],
        )
        .await
    }

    pub async fn get_alerts(&self) -> Result<Vec<Alert>> {
        get_json(&self.client, endpoint(&self.base, "api/alerts")?, &[]).await
    }

    pub async fn get_earthquakes(
        &self,
        center: Coordinates,
        radius_km: f64,
    ) -> Result<Vec<Earthquake>> {
        let url = endpoint(&self.base, "api/earthquakes")?;
        let (lat, lon, radius_km) = (
            center.lat().to_string(),
            center.lon().to_string(),
            radius_km.to_string(),
        );
        get_json(
            &self.client,
            url,
            &[("lat", &lat), ("lon", &lon), ("radius_km", &radius_km)],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn client(server: &mockito::Server) -> WeatherClient {
        WeatherClient::new(Client::new(), server.url().parse().unwrap())
    }

    #[tokio::test]
    async fn encodes_location_in_nowcast_query() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(Matcher::UrlEncoded("location".into(), "Ål & Hol".into()))
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let nowcasts = client(&server).get_nowcasts("Ål & Hol").await.unwrap();

        assert!(nowcasts.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn decodes_backend_nowcasts() {
        let mut server = mockito::Server::new_async().await;
        let body = serde_json::json!([{ "met": {
            "time": "2023-08-14T18:16:07Z",
            "location": { "lon": 10.4, "lat": 63.4 },
            "description": "cloudy",
            "air_temperature": 17.7,
            "relative_humidity": 80.5,
            "precipitation_rate": 0.0,
            "precipitation_amount": 0.0,
            "wind_speed": 2.7,
            "wind_speed_gust": 6.1,
            "wind_from_direction": 294.4
        }}]);
        let _mock = server
            .mock("GET", "/api/nowcasts")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let nowcasts = client(&server).get_nowcasts("Trondheim").await.unwrap();

        assert!(matches!(nowcasts.as_slice(), [Nowcast::Met(_)]));
    }

    #[tokio::test]
    async fn sends_earthquake_area_as_query() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/earthquakes")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("lat".into(), "36.2".into()),
                Matcher::UrlEncoded("lon".into(), "138.25".into()),
                Matcher::UrlEncoded("radius_km".into(), "2000".into()),
            ]))
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        client(&server)
            .get_earthquakes(Coordinates::new(138.25, 36.2).unwrap(), 2000.0)
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn reports_http_errors() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/alerts")
            .with_status(502)
            .create_async()
            .await;

        assert!(client(&server).get_alerts().await.is_err());
    }
}
