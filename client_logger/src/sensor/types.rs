use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub struct SensorIds {
    pub temperature: i32,
    pub humidity: i32,
    pub wind_speed: i32,
    pub wind_deg: i32,
    pub precipitation_rate: i32,
    pub precipitation_amount: i32,
    pub wind_speed_gust: i32,
    pub feels_like: i32,
    pub pressure: i32,
    pub clouds: i32,
    pub visibility: i32,
    pub lon: i32,
    pub lat: i32,
    pub alert_count: i32,
    pub earthquake_depth: i32,
    pub earthquake_magnitude: i32,
    pub earthquake_significance: i32,
    pub earthquake_tsunami: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Sensor {
    #[serde(skip_serializing)]
    pub id: i32,
    pub name: String,
    pub unit: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, Arbitrary, Gen};

    impl Arbitrary for Sensor {
        fn arbitrary(g: &mut Gen) -> Self {
            Sensor {
                id: i32::arbitrary(g),
                name: String::arbitrary(g),
                unit: String::arbitrary(g),
            }
        }
    }

    impl Arbitrary for SensorIds {
        fn arbitrary(g: &mut Gen) -> Self {
            SensorIds {
                temperature: i32::arbitrary(g),
                humidity: i32::arbitrary(g),
                wind_speed: i32::arbitrary(g),
                wind_deg: i32::arbitrary(g),
                precipitation_rate: i32::arbitrary(g),
                precipitation_amount: i32::arbitrary(g),
                wind_speed_gust: i32::arbitrary(g),
                feels_like: i32::arbitrary(g),
                pressure: i32::arbitrary(g),
                clouds: i32::arbitrary(g),
                visibility: i32::arbitrary(g),
                lon: i32::arbitrary(g),
                lat: i32::arbitrary(g),
                alert_count: i32::arbitrary(g),
                earthquake_depth: i32::arbitrary(g),
                earthquake_magnitude: i32::arbitrary(g),
                earthquake_significance: i32::arbitrary(g),
                earthquake_tsunami: i32::arbitrary(g),
            }
        }
    }

    quickcheck! {
        fn prop_identical_sensors_are_equal(sensor: Sensor) -> bool {
            sensor == sensor.clone()
        }

        fn prop_identical_sensor_ids_are_equal(ids: SensorIds) -> bool {
            ids == ids.clone()
        }

        fn prop_serialization_excludes_id(sensor: Sensor) -> bool {
            let json = serde_json::to_string(&sensor).unwrap();
            !json.contains("\"id\"")
        }

        fn prop_serialization_preserves_name_and_unit(sensor: Sensor) -> bool {
            let json = serde_json::to_string(&sensor).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            parsed["name"] == sensor.name && parsed["unit"] == sensor.unit
        }

        fn prop_deserialization_round_trips(sensor: Sensor) -> bool {
            let json = format!(
                r#"{{"id":{},"name":{},"unit":{}}}"#,
                sensor.id,
                serde_json::to_string(&sensor.name).unwrap(),
                serde_json::to_string(&sensor.unit).unwrap()
            );
            let parsed: Sensor = serde_json::from_str(&json).unwrap();
            parsed == sensor
        }
    }
}
