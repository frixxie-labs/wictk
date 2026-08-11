use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::device::DeviceId;
use crate::sensor::SensorIds;

pub mod storage_client;

pub use storage_client::StorageClient;

pub trait StorageApi {
    async fn store_met_nowcast(
        &self,
        url: &str,
        met_nowcast: &wictk_core::MetNowcast,
        device_id: &DeviceId,
        sensor_ids: &SensorIds,
    ) -> Result<()>;

    async fn store_openweather_nowcast(
        &self,
        url: &str,
        open_weather_nowcast: &wictk_core::OpenWeatherNowcast,
        device_id: &DeviceId,
        sensor_ids: &SensorIds,
    ) -> Result<()>;

    async fn store_lightnings(
        &self,
        url: &str,
        device_id: &DeviceId,
        lon_id: i32,
        lat_id: i32,
        lightnings: &[wictk_core::Lightning],
    ) -> Result<()>;

    async fn store_alert_count(
        &self,
        url: &str,
        timestamp: DateTime<Utc>,
        device_id: &DeviceId,
        alert_count_sensor_id: i32,
        alert_count: usize,
    ) -> Result<()>;

    async fn store_earthquakes(
        &self,
        url: &str,
        device_id: &DeviceId,
        sensor_ids: &SensorIds,
        earthquakes: &[wictk_core::Earthquake],
    ) -> Result<()>;
}
