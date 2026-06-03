use anyhow::Result;

pub mod sensor_client;
pub mod types;

pub use sensor_client::SensorClient;
pub use types::{Sensor, SensorIds};

pub trait SensorApi {
    async fn get_sensors(&mut self, url: &str) -> Result<Vec<Sensor>>;
    async fn setup_sensor(
        &mut self,
        url: &str,
        sensor_name: &str,
        sensor_unit: &str,
    ) -> Result<i32>;
    async fn setup_sensors(&mut self, url: &str) -> Result<SensorIds>;
}
