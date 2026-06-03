use anyhow::Result;

pub mod device_client;
pub mod types;

pub use device_client::DeviceClient;
pub use types::Device;

pub type DeviceId = i32;

pub trait DeviceApi {
    async fn get_devices(&mut self, url: &str) -> Result<Vec<Device>>;
    async fn setup_device(
        &mut self,
        url: &str,
        device_name: &str,
        device_location: &str,
    ) -> Result<DeviceId>;
}
