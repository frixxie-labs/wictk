use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use clap::Parser;
use device::{DeviceId, DeviceRegistry, NewDevice};
use registry::Registry;
use sensor::{SensorIds, SensorRegistry};
use storage::StorageClient;
use weather::WeatherClient;
use wictk_core::{Coordinates, Earthquake, Lightning, Nowcast};

mod api;
mod cli;
mod device;
mod measurement;
mod registry;
mod sensor;
mod storage;
mod weather;

const EARTHQUAKE_REGION: &str = "Japan";
const EARTHQUAKE_REGION_CENTER: (f32, f32) = (138.2529, 36.2048);
const EARTHQUAKE_REGION_RADIUS_KM: f64 = 2000.0;

fn recent_window() -> TimeDelta {
    TimeDelta::minutes(10)
}

/// Earthquakes from the last ten minutes, inclusive, excluding future timestamps.
fn recent_earthquakes(earthquakes: Vec<Earthquake>, now: DateTime<Utc>) -> Vec<Earthquake> {
    let cutoff = now - recent_window();
    earthquakes
        .into_iter()
        .filter(|earthquake| (cutoff..=now).contains(&earthquake.time))
        .collect()
}

/// Lightning younger than ten minutes.
fn recent_lightnings(lightnings: Vec<Lightning>, now: DateTime<Utc>) -> Vec<Lightning> {
    let cutoff = now - recent_window();
    lightnings
        .into_iter()
        .filter(|lightning| lightning.time > cutoff)
        .collect()
}

struct NowcastMeasurements {
    measurements: Vec<measurement::NewMeasurement>,
    met_count: usize,
    openweather_count: usize,
}

fn nowcast_measurements(
    nowcasts: &[Nowcast],
    met_device: DeviceId,
    openweather_device: DeviceId,
    sensors: &SensorIds,
) -> NowcastMeasurements {
    nowcasts.iter().fold(
        NowcastMeasurements {
            measurements: Vec::new(),
            met_count: 0,
            openweather_count: 0,
        },
        |mut acc, nowcast| {
            match nowcast {
                Nowcast::Met(met) => {
                    acc.met_count += 1;
                    acc.measurements
                        .extend(measurement::met_nowcast(met, met_device, sensors));
                }
                Nowcast::OpenWeather(openweather) => {
                    acc.openweather_count += 1;
                    acc.measurements.extend(measurement::openweather_nowcast(
                        openweather,
                        openweather_device,
                        sensors,
                    ));
                }
            }
            acc
        },
    )
}

struct Logger {
    weather: WeatherClient,
    storage: StorageClient,
    devices: DeviceRegistry,
    sensors: SensorIds,
}

impl Logger {
    async fn log_alert_count(&self, device: DeviceId) -> Result<()> {
        let alerts = self.weather.get_alerts().await?;
        tracing::info!("Backend returned {} weather alerts", alerts.len());
        self.storage
            .store(&measurement::alert_count(
                Utc::now(),
                alerts.len(),
                device,
                &self.sensors,
            ))
            .await
    }

    async fn log_earthquakes(&self, device: DeviceId) -> Result<()> {
        let (lon, lat) = EARTHQUAKE_REGION_CENTER;
        let earthquakes = self
            .weather
            .get_earthquakes(Coordinates::new(lon, lat)?, EARTHQUAKE_REGION_RADIUS_KM)
            .await?;
        let earthquakes = recent_earthquakes(earthquakes, Utc::now());
        if earthquakes.is_empty() {
            tracing::info!("No earthquakes found near {EARTHQUAKE_REGION} in the last 10 minutes");
            return Ok(());
        }
        tracing::warn!(
            earthquake_count = earthquakes.len(),
            earthquakes = ?earthquakes,
            "Earthquakes found near {EARTHQUAKE_REGION} in the last 10 minutes"
        );
        self.storage
            .store(&measurement::earthquakes(
                &earthquakes,
                device,
                &self.sensors,
            ))
            .await
    }

    async fn log_location(&mut self, location: &str) -> Result<()> {
        let start = Instant::now();
        let nowcasts = self.weather.get_nowcasts(location).await?;
        tracing::info!(
            "Retrieved {} nowcasts for {} in {:.2}s",
            nowcasts.len(),
            location,
            start.elapsed().as_secs_f64()
        );
        if nowcasts.is_empty() {
            tracing::warn!("No nowcast data available for location: {}", location);
            return Ok(());
        }

        let met_device = self
            .devices
            .ensure(&NewDevice::new("wictk_met", location))
            .await?;
        let openweather_device = self
            .devices
            .ensure(&NewDevice::new("wictk_opm", location))
            .await?;

        let batch = nowcast_measurements(&nowcasts, met_device, openweather_device, &self.sensors);
        let start = Instant::now();
        self.storage.store(&batch.measurements).await?;
        tracing::info!(
            "Stored {} nowcasts for {} ({} MET, {} OpenWeatherMap) in {:.2}s",
            nowcasts.len(),
            location,
            batch.met_count,
            batch.openweather_count,
            start.elapsed().as_secs_f64()
        );
        Ok(())
    }

    async fn log_lightning(&self, device: DeviceId) -> Result<()> {
        let lightnings = self.weather.get_lightnings().await?;
        let total = lightnings.len();
        let recent = recent_lightnings(lightnings, Utc::now());
        tracing::info!(
            "Filtered {} recent lightnings from {} total (within 10 minutes)",
            recent.len(),
            total
        );
        self.storage
            .store(&measurement::lightnings(&recent, device, &self.sensors))
            .await?;
        tracing::info!("Stored {} lightning records", recent.len());
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = cli::Opts::parse();
    cli::init_tracing(&opts)?;
    tracing::info!("Starting client logger with configuration: {:?}", opts);

    let client = reqwest::Client::new();
    let setup_start = Instant::now();
    let sensors = SensorRegistry::new(
        client.clone(),
        api::endpoint(&opts.hemrs_url, "api/sensors")?,
    )
    .setup_sensors()
    .await?;
    let mut devices: DeviceRegistry = Registry::new(
        client.clone(),
        api::endpoint(&opts.hemrs_url, "api/devices")?,
    );
    let lightning_device = devices
        .ensure(&NewDevice::new("wictk_lightning", "Mobile"))
        .await?;
    let alerts_device = devices
        .ensure(&NewDevice::new("wictk_alerts", "Global"))
        .await?;
    let earthquake_device = devices
        .ensure(&NewDevice::new("wictk_earthquake", EARTHQUAKE_REGION))
        .await?;
    tracing::info!(
        "Global setup completed in {:.2}s",
        setup_start.elapsed().as_secs_f64()
    );

    let mut logger = Logger {
        weather: WeatherClient::new(client.clone(), opts.service_url.clone()),
        storage: StorageClient::new(client, api::endpoint(&opts.hemrs_url, "api/measurements")?),
        devices,
        sensors,
    };

    if let Err(err) = logger.log_alert_count(alerts_device).await {
        tracing::warn!(error = ?err, "Failed to log weather alert count");
    }
    if let Err(err) = logger.log_earthquakes(earthquake_device).await {
        tracing::warn!(error = ?err, "Failed to log earthquakes near {EARTHQUAKE_REGION}");
    }
    for location in &opts.locations {
        if let Err(err) = logger.log_location(location).await {
            tracing::error!(error = ?err, "Failed to log nowcasts for {location}");
        }
    }

    if !opts.store_lightning {
        tracing::info!("Lightning storage disabled - skipping lightning data");
        return Ok(());
    }
    logger.log_lightning(lightning_device).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use geo::Point;
    use quickcheck::quickcheck;
    use wictk_core::EarthquakeStatus;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn earthquake(id: &str, time: DateTime<Utc>) -> Earthquake {
        Earthquake {
            id: id.to_string(),
            magnitude: Some(1.0),
            place: None,
            time,
            updated: time,
            coordinates: Coordinates::new(138.2529, 36.2048).unwrap(),
            depth_km: 1.0,
            significance: None,
            alert_level: None,
            status: EarthquakeStatus::Reviewed,
            tsunami: false,
            details_url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn retains_only_earthquakes_from_the_last_ten_minutes() {
        let now = now();
        let earthquakes = vec![
            earthquake("old", now - Duration::minutes(10) - Duration::seconds(1)),
            earthquake("boundary", now - Duration::minutes(10)),
            earthquake("recent", now - Duration::minutes(1)),
            earthquake("future", now + Duration::seconds(1)),
        ];

        let recent = recent_earthquakes(earthquakes, now);

        let ids: Vec<&str> = recent.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["boundary", "recent"]);
    }

    quickcheck! {
        fn prop_recent_lightnings_are_younger_than_ten_minutes(offsets: Vec<i32>) -> bool {
            let now = now();
            let strikes: Vec<Lightning> = offsets
                .iter()
                .map(|offset| Lightning::new(Point::new(0.0, 0.0), now - Duration::seconds(*offset as i64 % 3600), 1))
                .collect();
            let total = strikes.len();

            let recent = recent_lightnings(strikes, now);

            recent.iter().all(|strike| now - strike.time < recent_window())
                && recent.len()
                    == offsets.iter().filter(|o| ((**o as i64) % 3600) < 600).count()
                && recent.len() <= total
        }
    }

    #[test]
    fn counts_nowcasts_per_provider_and_builds_their_measurements() {
        let met: Nowcast = serde_json::from_value(serde_json::json!({ "met": {
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
        }}))
        .unwrap();
        let sensors = SensorIds::from_fn(|kind| sensor::SensorId(kind as i32));

        let batch = nowcast_measurements(&[met.clone(), met], DeviceId(1), DeviceId(2), &sensors);

        assert_eq!(batch.met_count, 2);
        assert_eq!(batch.openweather_count, 0);
        assert_eq!(batch.measurements.len(), 14);
        assert!(batch.measurements.iter().all(|m| m.device == DeviceId(1)));
    }
}
