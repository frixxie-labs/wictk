use serde::{Deserialize, Serialize};

use crate::registry::{Registered, Registry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub i32);

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub location: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct NewDevice {
    pub name: String,
    pub location: String,
}

impl NewDevice {
    pub fn new(name: &str, location: &str) -> Self {
        Self {
            name: name.to_string(),
            location: location.to_string(),
        }
    }
}

impl Registered for Device {
    type Key = (String, String);
    type Id = DeviceId;
    type New = NewDevice;

    fn key(&self) -> Self::Key {
        (self.name.clone(), self.location.clone())
    }

    fn id(&self) -> DeviceId {
        self.id
    }

    fn new_key(new: &NewDevice) -> Self::Key {
        (new.name.clone(), new.location.clone())
    }
}

pub type DeviceRegistry = Registry<Device>;

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use serde_json::json;

    fn registry(server: &mockito::Server) -> DeviceRegistry {
        Registry::new(reqwest::Client::new(), server.url().parse().unwrap())
    }

    #[tokio::test]
    async fn finds_existing_device_and_caches_it() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_body(r#"[{"id": 1, "name": "wictk_met", "location": "Trondheim"}]"#)
            .expect(1)
            .create_async()
            .await;
        let mut devices = registry(&server);

        let first = devices
            .ensure(&NewDevice::new("wictk_met", "Trondheim"))
            .await;
        let second = devices
            .ensure(&NewDevice::new("wictk_met", "Trondheim"))
            .await;

        assert_eq!(first.unwrap(), DeviceId(1));
        assert_eq!(second.unwrap(), DeviceId(1));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn distinguishes_devices_by_location() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_body(
                r#"[{"id": 1, "name": "wictk_met", "location": "Oslo"},
                    {"id": 2, "name": "wictk_met", "location": "Bergen"}]"#,
            )
            .create_async()
            .await;
        let mut devices = registry(&server);

        let bergen = devices.ensure(&NewDevice::new("wictk_met", "Bergen")).await;

        assert_eq!(bergen.unwrap(), DeviceId(2));
    }

    #[tokio::test]
    async fn creates_missing_device_without_an_id() {
        let mut server = mockito::Server::new_async().await;
        let _empty = server
            .mock("GET", "/")
            .with_status(200)
            .with_body("[]")
            .expect(1)
            .create_async()
            .await;
        let create = server
            .mock("POST", "/")
            .match_body(Matcher::Json(
                json!({"name": "wictk_met", "location": "Oslo"}),
            ))
            .with_status(201)
            .create_async()
            .await;
        let _created = server
            .mock("GET", "/")
            .with_status(200)
            .with_body(r#"[{"id": 7, "name": "wictk_met", "location": "Oslo"}]"#)
            .create_async()
            .await;
        let mut devices = registry(&server);

        let id = devices.ensure(&NewDevice::new("wictk_met", "Oslo")).await;

        assert_eq!(id.unwrap(), DeviceId(7));
        create.assert_async().await;
    }

    #[tokio::test]
    async fn fails_when_creation_is_rejected() {
        let mut server = mockito::Server::new_async().await;
        let _get = server
            .mock("GET", "/")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;
        let _post = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;
        let mut devices = registry(&server);

        assert!(devices
            .ensure(&NewDevice::new("wictk_met", "Oslo"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn fails_on_invalid_json_and_keeps_cache_empty() {
        let mut server = mockito::Server::new_async().await;
        let _get = server
            .mock("GET", "/")
            .with_status(200)
            .with_body("not json")
            .create_async()
            .await;
        let mut devices = registry(&server);

        assert!(devices.refresh().await.is_err());
        assert_eq!(
            devices.cached(&("wictk_met".to_string(), "Oslo".to_string())),
            None
        );
    }
}
