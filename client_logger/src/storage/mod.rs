use anyhow::Result;
use reqwest::{Client, Url};

use crate::api::post_json;
use crate::measurement::NewMeasurement;

/// Writes measurement batches to hemrs.
pub struct StorageClient {
    client: Client,
    url: Url,
}

impl StorageClient {
    pub fn new(client: Client, url: Url) -> Self {
        Self { client, url }
    }

    pub async fn store(&self, measurements: &[NewMeasurement]) -> Result<()> {
        if measurements.is_empty() {
            return Ok(());
        }
        post_json(&self.client, self.url.clone(), measurements).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceId;
    use crate::sensor::SensorId;
    use chrono::Utc;

    fn measurement() -> NewMeasurement {
        NewMeasurement {
            timestamp: Utc::now(),
            device: DeviceId(1),
            sensor: SensorId(2),
            measurement: 3.0,
        }
    }

    #[tokio::test]
    async fn posts_measurements() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"[{"device": 1, "sensor": 2, "measurement": 3.0}]"#.to_string(),
            ))
            .with_status(201)
            .create_async()
            .await;
        let storage = StorageClient::new(Client::new(), server.url().parse().unwrap());

        storage.store(&[measurement()]).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn skips_empty_batches() {
        let mut server = mockito::Server::new_async().await;
        let mock = server.mock("POST", "/").expect(0).create_async().await;
        let storage = StorageClient::new(Client::new(), server.url().parse().unwrap());

        storage.store(&[]).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn reports_rejected_batches() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;
        let storage = StorageClient::new(Client::new(), server.url().parse().unwrap());

        assert!(storage.store(&[measurement()]).await.is_err());
    }
}
