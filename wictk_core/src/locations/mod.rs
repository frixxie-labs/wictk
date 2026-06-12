mod city;
mod coordinates;
mod location;

pub use city::City;
pub use coordinates::Coordinates;
pub use coordinates::CoordinatesAsString;
pub use location::OpenWeatherMapLocation;

#[cfg(test)]
mod tests {
    use quickcheck::{quickcheck, TestResult};

    use crate::locations::{city::City, coordinates::Coordinates, CoordinatesAsString};

    quickcheck! {
        fn prop_coordinates_new_preserves_fields(lon: f32, lat: f32) -> bool {
            let location = Coordinates::new(lon, lat);

            location.lon.to_bits() == lon.to_bits() && location.lat.to_bits() == lat.to_bits()
        }

        fn prop_coordinates_json_round_trips(lon: f32, lat: f32) -> TestResult {
            if !lon.is_finite() || !lat.is_finite() {
                return TestResult::discard();
            }

            let location = Coordinates::new(lon, lat);
            let json = serde_json::to_string(&location).unwrap();
            let parsed: Coordinates = serde_json::from_str(&json).unwrap();

            TestResult::from_bool(parsed == location)
        }

        fn prop_coordinates_as_string_matches_f32_parse(lon: String, lat: String) -> bool {
            let parsed = Coordinates::try_from(CoordinatesAsString {
                lon: lon.clone(),
                lat: lat.clone(),
            });

            match (lon.parse::<f32>(), lat.parse::<f32>(), parsed) {
                (Ok(expected_lon), Ok(expected_lat), Ok(actual)) => {
                    actual.lon.to_bits() == expected_lon.to_bits()
                        && actual.lat.to_bits() == expected_lat.to_bits()
                }
                (Err(_), _, Err(_)) | (_, Err(_), Err(_)) => true,
                _ => false,
            }
        }

        fn prop_city_json_round_trips(location: String) -> bool {
            let city = City { location };
            let json = serde_json::to_string(&city).unwrap();
            let parsed: City = serde_json::from_str(&json).unwrap();

            parsed == city
        }
    }
}
