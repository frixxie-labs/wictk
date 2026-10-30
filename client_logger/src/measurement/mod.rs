use chrono::{DateTime, Utc};
use serde::Serialize;
use wictk_core::{MetNowcast, OpenWeatherNowcast};

use crate::device::DeviceId;
use crate::sensor::{SensorIds, SensorKind};

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct NewMeasurement {
    pub timestamp: DateTime<Utc>,
    pub device: DeviceId,
    pub sensor: crate::sensor::SensorId,
    pub measurement: f32,
}

/// Builds measurements that share a timestamp and device.
fn readings(
    timestamp: DateTime<Utc>,
    device: DeviceId,
    sensors: &SensorIds,
    values: impl IntoIterator<Item = (SensorKind, f32)>,
) -> Vec<NewMeasurement> {
    values
        .into_iter()
        .map(|(kind, measurement)| NewMeasurement {
            timestamp,
            device,
            sensor: sensors.get(kind),
            measurement,
        })
        .collect()
}

pub fn met_nowcast(
    nowcast: &MetNowcast,
    device: DeviceId,
    sensors: &SensorIds,
) -> Vec<NewMeasurement> {
    readings(
        nowcast.time,
        device,
        sensors,
        [
            (SensorKind::Temperature, nowcast.air_temperature),
            (SensorKind::Humidity, nowcast.relative_humidity),
            (SensorKind::WindSpeed, nowcast.wind_speed),
            (SensorKind::WindDeg, nowcast.wind_from_direction),
            (SensorKind::PrecipitationRate, nowcast.precipitation_rate),
            (
                SensorKind::PrecipitationAmount,
                nowcast.precipitation_amount,
            ),
            (SensorKind::WindSpeedGust, nowcast.wind_speed_gust),
        ],
    )
}

pub fn openweather_nowcast(
    nowcast: &OpenWeatherNowcast,
    device: DeviceId,
    sensors: &SensorIds,
) -> Vec<NewMeasurement> {
    readings(
        nowcast.dt,
        device,
        sensors,
        [
            (SensorKind::Temperature, nowcast.temp),
            (SensorKind::Humidity, nowcast.humidity as f32),
            (SensorKind::WindSpeed, nowcast.wind_speed),
            (SensorKind::WindDeg, nowcast.wind_deg as f32),
            (SensorKind::FeelsLike, nowcast.feels_like),
            (SensorKind::Pressure, nowcast.pressure as f32),
            (SensorKind::Clouds, nowcast.clouds as f32),
            (SensorKind::Visibility, nowcast.visibility as f32),
        ],
    )
}

pub fn alert_count(
    timestamp: DateTime<Utc>,
    count: usize,
    device: DeviceId,
    sensors: &SensorIds,
) -> Vec<NewMeasurement> {
    readings(
        timestamp,
        device,
        sensors,
        [(SensorKind::AlertCount, count as f32)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensor::SensorId;
    use chrono::TimeZone;

    fn sensors() -> SensorIds {
        SensorIds::from_fn(|kind| SensorId(kind as i32 + 1))
    }

    fn time(secs: u32) -> DateTime<Utc> {
        Utc.timestamp_opt(secs as i64, 0).unwrap()
    }

    #[test]
    fn serializes_ids_as_plain_numbers() {
        let measurement = &alert_count(time(0), 3, DeviceId(2), &sensors())[0];

        let json = serde_json::to_value(measurement).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "timestamp": "1970-01-01T00:00:00Z",
                "device": 2,
                "sensor": SensorKind::AlertCount as i32 + 1,
                "measurement": 3.0
            })
        );
    }
}
