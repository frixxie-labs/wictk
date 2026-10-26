use chrono::{DateTime, Utc};
use serde::Serialize;
use wictk_core::{Earthquake, Lightning, MetNowcast, OpenWeatherNowcast};

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

pub fn lightnings(
    lightnings: &[Lightning],
    device: DeviceId,
    sensors: &SensorIds,
) -> Vec<NewMeasurement> {
    lightnings
        .iter()
        .flat_map(|lightning| {
            readings(
                lightning.time,
                device,
                sensors,
                [
                    (SensorKind::Lat, lightning.location.y() as f32),
                    (SensorKind::Lon, lightning.location.x() as f32),
                ],
            )
        })
        .collect()
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

pub fn earthquakes(
    earthquakes: &[Earthquake],
    device: DeviceId,
    sensors: &SensorIds,
) -> Vec<NewMeasurement> {
    earthquakes
        .iter()
        .flat_map(|earthquake| {
            let required = [
                (SensorKind::EarthquakeDepth, earthquake.depth_km as f32),
                (SensorKind::Lon, earthquake.coordinates.lon()),
                (SensorKind::Lat, earthquake.coordinates.lat()),
                (SensorKind::EarthquakeTsunami, f32::from(earthquake.tsunami)),
            ];
            let optional = [
                earthquake
                    .magnitude
                    .map(|magnitude| (SensorKind::EarthquakeMagnitude, magnitude as f32)),
                earthquake
                    .significance
                    .map(|significance| (SensorKind::EarthquakeSignificance, significance as f32)),
            ];
            readings(
                earthquake.time,
                device,
                sensors,
                required.into_iter().chain(optional.into_iter().flatten()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensor::SensorId;
    use chrono::TimeZone;
    use geo::Point;
    use quickcheck::quickcheck;
    use wictk_core::{Coordinates, EarthquakeStatus};

    fn sensors() -> SensorIds {
        SensorIds::from_fn(|kind| SensorId(kind as i32 + 1))
    }

    fn time(secs: u32) -> DateTime<Utc> {
        Utc.timestamp_opt(secs as i64, 0).unwrap()
    }

    fn earthquake(secs: u32, magnitude: Option<f64>, significance: Option<u64>) -> Earthquake {
        Earthquake {
            id: secs.to_string(),
            magnitude,
            place: None,
            time: time(secs),
            updated: time(secs),
            coordinates: Coordinates::new(138.25, 36.2).unwrap(),
            depth_km: 10.0,
            significance,
            alert_level: None,
            status: EarthquakeStatus::Reviewed,
            tsunami: true,
            details_url: String::new(),
        }
    }

    quickcheck! {
        fn prop_two_measurements_per_lightning_at_its_time(strikes: Vec<(u32, i16, i16)>) -> bool {
            let strikes: Vec<Lightning> = strikes
                .into_iter()
                .map(|(secs, lon, lat)| Lightning::new(Point::new(lon as f64 / 200.0, lat as f64 / 400.0), time(secs), 1))
                .collect();

            let measurements = lightnings(&strikes, DeviceId(3), &sensors());

            measurements.len() == 2 * strikes.len()
                && measurements
                    .chunks(2)
                    .zip(&strikes)
                    .all(|(pair, strike)| pair.iter().all(|m| m.timestamp == strike.time && m.device == DeviceId(3)))
        }

        fn prop_earthquakes_produce_four_to_six_measurements_each(events: Vec<(u32, Option<u8>, Option<u16>)>) -> bool {
            let earthquakes: Vec<Earthquake> = events
                .iter()
                .map(|(secs, magnitude, significance)| {
                    earthquake(*secs, magnitude.map(f64::from), significance.map(u64::from))
                })
                .collect();
            let expected: usize = events
                .iter()
                .map(|(_, magnitude, significance)| 4 + magnitude.is_some() as usize + significance.is_some() as usize)
                .sum();

            super::earthquakes(&earthquakes, DeviceId(1), &sensors()).len() == expected
        }
    }

    #[test]
    fn maps_earthquake_values_to_their_sensors() {
        let measurements =
            super::earthquakes(&[earthquake(0, Some(5.5), None)], DeviceId(1), &sensors());

        let value_of = |kind: SensorKind| {
            measurements
                .iter()
                .find(|m| m.sensor == sensors().get(kind))
                .map(|m| m.measurement)
        };
        assert_eq!(value_of(SensorKind::EarthquakeDepth), Some(10.0));
        assert_eq!(value_of(SensorKind::Lon), Some(138.25));
        assert_eq!(value_of(SensorKind::Lat), Some(36.2));
        assert_eq!(value_of(SensorKind::EarthquakeTsunami), Some(1.0));
        assert_eq!(value_of(SensorKind::EarthquakeMagnitude), Some(5.5));
        assert_eq!(value_of(SensorKind::EarthquakeSignificance), None);
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
