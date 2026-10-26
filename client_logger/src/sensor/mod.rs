use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::registry::{Registered, Registry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SensorId(pub i32);

/// Every sensor the logger writes measurements for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SensorKind {
    Temperature,
    Humidity,
    WindSpeed,
    WindDeg,
    PrecipitationRate,
    PrecipitationAmount,
    WindSpeedGust,
    FeelsLike,
    Pressure,
    Clouds,
    Visibility,
    Lon,
    Lat,
    AlertCount,
    EarthquakeDepth,
    EarthquakeMagnitude,
    EarthquakeSignificance,
    EarthquakeTsunami,
}

impl SensorKind {
    pub const ALL: [SensorKind; 18] = [
        SensorKind::Temperature,
        SensorKind::Humidity,
        SensorKind::WindSpeed,
        SensorKind::WindDeg,
        SensorKind::PrecipitationRate,
        SensorKind::PrecipitationAmount,
        SensorKind::WindSpeedGust,
        SensorKind::FeelsLike,
        SensorKind::Pressure,
        SensorKind::Clouds,
        SensorKind::Visibility,
        SensorKind::Lon,
        SensorKind::Lat,
        SensorKind::AlertCount,
        SensorKind::EarthquakeDepth,
        SensorKind::EarthquakeMagnitude,
        SensorKind::EarthquakeSignificance,
        SensorKind::EarthquakeTsunami,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SensorKind::Temperature => "temperature",
            SensorKind::Humidity => "humidity",
            SensorKind::WindSpeed => "wind_speed",
            SensorKind::WindDeg => "wind_deg",
            SensorKind::PrecipitationRate => "precipitation_rate",
            SensorKind::PrecipitationAmount => "precipitation_amount",
            SensorKind::WindSpeedGust => "wind_speed_gust",
            SensorKind::FeelsLike => "feels_like",
            SensorKind::Pressure => "pressure",
            SensorKind::Clouds => "clouds",
            SensorKind::Visibility => "visibility",
            SensorKind::Lon => "lon",
            SensorKind::Lat => "lat",
            SensorKind::AlertCount => "alert_count",
            SensorKind::EarthquakeDepth => "earthquake_depth",
            SensorKind::EarthquakeMagnitude => "earthquake_magnitude",
            SensorKind::EarthquakeSignificance => "earthquake_significance",
            SensorKind::EarthquakeTsunami => "earthquake_tsunami",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            SensorKind::Temperature | SensorKind::FeelsLike => "°C",
            SensorKind::Humidity | SensorKind::Clouds => "%",
            SensorKind::WindSpeed | SensorKind::WindSpeedGust => "m/s",
            SensorKind::WindDeg | SensorKind::Lon | SensorKind::Lat => "°",
            SensorKind::PrecipitationRate => "mm/h",
            SensorKind::PrecipitationAmount => "mm",
            SensorKind::Pressure => "hPa",
            SensorKind::Visibility => "m",
            SensorKind::AlertCount => "count",
            SensorKind::EarthquakeDepth => "km",
            SensorKind::EarthquakeMagnitude => "magnitude",
            SensorKind::EarthquakeSignificance => "score",
            SensorKind::EarthquakeTsunami => "boolean",
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|kind| *kind == self)
            .expect("ALL contains every kind")
    }
}

/// Resolved ids for every [`SensorKind`]; complete by construction.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorIds([SensorId; SensorKind::ALL.len()]);

impl SensorIds {
    pub fn get(&self, kind: SensorKind) -> SensorId {
        self.0[kind.index()]
    }

    #[cfg(test)]
    pub fn from_fn(f: impl Fn(SensorKind) -> SensorId) -> Self {
        Self(SensorKind::ALL.map(f))
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Sensor {
    pub id: SensorId,
    pub name: String,
    pub unit: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct NewSensor {
    pub name: String,
    pub unit: String,
}

impl From<SensorKind> for NewSensor {
    fn from(kind: SensorKind) -> Self {
        Self {
            name: kind.name().to_string(),
            unit: kind.unit().to_string(),
        }
    }
}

impl Registered for Sensor {
    type Key = String;
    type Id = SensorId;
    type New = NewSensor;

    fn key(&self) -> String {
        self.name.clone()
    }

    fn id(&self) -> SensorId {
        self.id
    }

    fn new_key(new: &NewSensor) -> String {
        new.name.clone()
    }
}

pub type SensorRegistry = Registry<Sensor>;

impl SensorRegistry {
    /// Ensures every sensor kind exists and returns their ids.
    pub async fn setup_sensors(&mut self) -> Result<SensorIds> {
        let mut ids = [SensorId(0); SensorKind::ALL.len()];
        for (slot, kind) in ids.iter_mut().zip(SensorKind::ALL) {
            *slot = self.ensure(&kind.into()).await?;
        }
        Ok(SensorIds(ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, Arbitrary, Gen};
    use std::collections::HashSet;

    impl Arbitrary for SensorKind {
        fn arbitrary(g: &mut Gen) -> Self {
            *g.choose(&SensorKind::ALL).unwrap()
        }
    }

    quickcheck! {
        fn prop_get_returns_the_id_assigned_to_each_kind(kind: SensorKind) -> bool {
            let ids = SensorIds::from_fn(|kind| SensorId(kind.index() as i32 * 10));
            ids.get(kind) == SensorId(kind.index() as i32 * 10)
        }
    }

    #[test]
    fn sensor_names_are_unique() {
        let names: HashSet<_> = SensorKind::ALL.iter().map(|kind| kind.name()).collect();
        assert_eq!(names.len(), SensorKind::ALL.len());
    }

    #[tokio::test]
    async fn resolves_every_sensor_with_a_single_fetch_when_all_exist() {
        let mut server = mockito::Server::new_async().await;
        let body: Vec<_> = SensorKind::ALL
            .iter()
            .enumerate()
            .map(|(i, kind)| serde_json::json!({"id": i + 100, "name": kind.name(), "unit": kind.unit()}))
            .collect();
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_body(serde_json::to_string(&body).unwrap())
            .expect(1)
            .create_async()
            .await;
        let mut registry = Registry::new(reqwest::Client::new(), server.url().parse().unwrap());

        let ids = registry.setup_sensors().await.unwrap();

        assert_eq!(ids.get(SensorKind::Temperature), SensorId(100));
        assert_eq!(ids.get(SensorKind::Lat), SensorId(112));
        assert_eq!(ids.get(SensorKind::EarthquakeTsunami), SensorId(117));
        mock.assert_async().await;
    }
}
