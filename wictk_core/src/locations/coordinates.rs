use geo::Point;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::location::LocationError;

/// A valid WGS84 position: longitude in [-180, 180] and latitude in [-90, 90].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(try_from = "RawCoordinates")]
pub struct Coordinates {
    lon: f32,
    lat: f32,
}

#[derive(Deserialize)]
struct RawCoordinates {
    lon: f32,
    lat: f32,
}

impl TryFrom<RawCoordinates> for Coordinates {
    type Error = LocationError;

    fn try_from(raw: RawCoordinates) -> Result<Self, Self::Error> {
        Self::new(raw.lon, raw.lat)
    }
}

impl Coordinates {
    pub fn new(lon: f32, lat: f32) -> Result<Self, LocationError> {
        if !(-180.0..=180.0).contains(&lon) {
            return Err(LocationError::LongitudeOutOfRange(lon));
        }
        if !(-90.0..=90.0).contains(&lat) {
            return Err(LocationError::LatitudeOutOfRange(lat));
        }
        Ok(Self { lon, lat })
    }

    pub fn lon(&self) -> f32 {
        self.lon
    }

    pub fn lat(&self) -> f32 {
        self.lat
    }
}

// Sound because construction rejects NaN; adding 0.0 folds -0.0 into 0.0 so hashing agrees with `==`.
impl Eq for Coordinates {}

impl std::hash::Hash for Coordinates {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.lon + 0.0).to_bits().hash(state);
        (self.lat + 0.0).to_bits().hash(state);
    }
}

impl std::fmt::Display for Coordinates {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{},{}", self.lon, self.lat)
    }
}

impl From<Coordinates> for Point {
    fn from(coordinates: Coordinates) -> Self {
        Point::new(coordinates.lon as f64, coordinates.lat as f64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct CoordinatesAsString {
    pub lon: String,
    pub lat: String,
}

impl TryFrom<CoordinatesAsString> for Coordinates {
    type Error = LocationError;

    fn try_from(value: CoordinatesAsString) -> Result<Self, Self::Error> {
        let lon = value
            .lon
            .parse::<f32>()
            .map_err(|_| LocationError::UnparsableLongitude(value.lon))?;
        let lat = value
            .lat
            .parse::<f32>()
            .map_err(|_| LocationError::UnparsableLatitude(value.lat))?;
        Self::new(lon, lat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    fn in_range(lon: f32, lat: f32) -> bool {
        (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)
    }

    quickcheck! {
        fn prop_new_accepts_exactly_the_valid_range(lon: f32, lat: f32) -> bool {
            Coordinates::new(lon, lat).is_ok() == in_range(lon, lat)
        }

        fn prop_json_round_trips(lon: f32, lat: f32) -> bool {
            match Coordinates::new(lon, lat) {
                Ok(coordinates) => {
                    let json = serde_json::to_string(&coordinates).unwrap();
                    serde_json::from_str::<Coordinates>(&json).unwrap() == coordinates
                }
                Err(_) => true,
            }
        }

        fn prop_deserialization_rejects_out_of_range(lon: f32, lat: f32) -> bool {
            let json = serde_json::json!({ "lon": lon, "lat": lat });
            let parsed = serde_json::from_value::<Coordinates>(json);
            !lon.is_finite() || !lat.is_finite() || parsed.is_ok() == in_range(lon, lat)
        }

        fn prop_display_parses_back(lon: f32, lat: f32) -> bool {
            match Coordinates::new(lon, lat) {
                Ok(coordinates) => {
                    let text = coordinates.to_string();
                    let (lon, lat) = text.split_once(',').unwrap();
                    let parsed = Coordinates::try_from(CoordinatesAsString {
                        lon: lon.to_string(),
                        lat: lat.to_string(),
                    });
                    parsed == Ok(coordinates)
                }
                Err(_) => true,
            }
        }
    }

    #[test]
    fn zero_and_negative_zero_are_equal_and_hash_equally() {
        use std::hash::{BuildHasher, RandomState};
        let hasher = RandomState::new();
        let positive = Coordinates::new(0.0, 0.0).unwrap();
        let negative = Coordinates::new(-0.0, -0.0).unwrap();

        assert_eq!(positive, negative);
        assert_eq!(hasher.hash_one(positive), hasher.hash_one(negative));
    }

    #[test]
    fn rejects_unparsable_strings() {
        let parsed = Coordinates::try_from(CoordinatesAsString {
            lon: "east".to_string(),
            lat: "10".to_string(),
        });

        assert_eq!(
            parsed,
            Err(LocationError::UnparsableLongitude("east".to_string()))
        );
    }
}
