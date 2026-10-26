use axum::{
    extract::{Query, State},
    Json,
};
use geo::{Distance, Haversine, Point};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};
use utoipa::{IntoParams, ToSchema};
use wictk_core::{City, Coordinates, CoordinatesAsString, OpenWeatherMapLocation};

use crate::AppState;

use super::error::ApplicationError;

/// Raw location query parameters shared by location-aware endpoints.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LocationParams {
    /// Location name (e.g., "Oslo")
    pub location: Option<String>,
    /// Latitude coordinate
    pub lat: Option<String>,
    /// Longitude coordinate
    pub lon: Option<String>,
}

/// A validated location: either a place name to geocode or explicit coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum LocationInput {
    City(String),
    Coordinates(Coordinates),
}

impl LocationParams {
    /// Parses the parameters; `Ok(None)` means no location was given.
    pub fn parse(self) -> Result<Option<LocationInput>, ApplicationError> {
        match (self.location, self.lat, self.lon) {
            (None, None, None) => Ok(None),
            (Some(location), None, None) => match location.trim() {
                "" => Err(bad_request("'location' must not be empty")),
                location => Ok(Some(LocationInput::City(location.to_string()))),
            },
            (None, Some(lat), Some(lon)) => Coordinates::try_from(CoordinatesAsString { lon, lat })
                .map(|coordinates| Some(LocationInput::Coordinates(coordinates)))
                .map_err(|err| bad_request(&err.to_string())),
            (Some(_), _, _) => Err(bad_request(
                "Provide either 'location' or 'lat' and 'lon', not both",
            )),
            (None, _, _) => Err(bad_request("Provide both 'lat' and 'lon'")),
        }
    }

    /// Like [`parse`](Self::parse), but a location is mandatory.
    pub fn parse_required(self) -> Result<LocationInput, ApplicationError> {
        self.parse()?.ok_or_else(|| {
            bad_request("Missing location parameter. Provide 'location' or 'lat' and 'lon'")
        })
    }
}

pub fn bad_request(message: &str) -> ApplicationError {
    ApplicationError::BadRequest(message.to_string())
}

pub fn within_radius_km(center: Point, point: Point, radius_km: f64) -> bool {
    Haversine.distance(center, point) <= radius_km * 1000.0
}

fn location_cache_key(location: &str) -> String {
    location.trim().to_lowercase()
}

pub async fn resolve_location(
    app_state: &AppState,
    input: LocationInput,
) -> Result<Coordinates, ApplicationError> {
    match input {
        LocationInput::Coordinates(coordinates) => Ok(coordinates),
        LocationInput::City(city) => lookup_location(app_state, &city)
            .await
            .map(|location| location.location),
    }
}

pub async fn lookup_location(
    app_state: &AppState,
    location: &str,
) -> Result<OpenWeatherMapLocation, ApplicationError> {
    let key = location_cache_key(location);
    if let Some(cached) = app_state.location_cache.get(&key).await {
        return Ok(cached);
    }

    let found = OpenWeatherMapLocation::fetch(
        &app_state.client,
        &app_state.provider_urls.openweathermap_geocoding,
        location,
        &app_state.openweathermap_apikey,
    )
    .await
    .map_err(|err| {
        error!(error = ?err, "Failed to get location data from OpenWeatherMap");
        ApplicationError::BadGateway("Failed to get location data from OpenWeatherMap".into())
    })?
    .into_iter()
    .next()
    .ok_or_else(|| ApplicationError::NotFound(format!("No location found for '{location}'")))?;

    app_state.location_cache.insert(key, found.clone()).await;
    Ok(found)
}

#[utoipa::path(
    get,
    path = "/api/geocoding",
    params(City),
    responses(
        (status = 200, description = "List of matching locations", body = Vec<OpenWeatherMapLocation>),
        (status = 400, description = "Missing location parameter"),
        (status = 502, description = "Geocoding provider failure", body = String)
    ),
    tag = "geocoding"
)]
pub async fn geocoding(
    State(app_state): State<AppState>,
    Query(query): Query<City>,
) -> Result<Json<Vec<OpenWeatherMapLocation>>, ApplicationError> {
    let locations = OpenWeatherMapLocation::fetch(
        &app_state.client,
        &app_state.provider_urls.openweathermap_geocoding,
        &query.location,
        &app_state.openweathermap_apikey,
    )
    .await
    .map_err(|err| {
        error!(error = ?err, "Failed to get geocoding data from OpenWeatherMap");
        ApplicationError::BadGateway("Failed to get geocoding data from OpenWeatherMap".into())
    })?;
    debug!("Returning {:?} for {:?}", &locations, &query);
    Ok(Json(locations))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_utils::{
        create_test_app, create_test_app_with_state, make_request, test_state,
    };
    use axum::http::StatusCode;
    use quickcheck::quickcheck;

    fn params(location: Option<&str>, lat: Option<&str>, lon: Option<&str>) -> LocationParams {
        LocationParams {
            location: location.map(str::to_string),
            lat: lat.map(str::to_string),
            lon: lon.map(str::to_string),
        }
    }

    #[test]
    fn parses_valid_inputs() {
        assert_eq!(params(None, None, None).parse(), Ok(None));
        assert_eq!(
            params(Some(" Oslo "), None, None).parse(),
            Ok(Some(LocationInput::City("Oslo".to_string())))
        );
        assert_eq!(
            params(None, Some("59.9"), Some("10.7")).parse(),
            Ok(Some(LocationInput::Coordinates(
                Coordinates::new(10.7, 59.9).unwrap()
            )))
        );
    }

    #[test]
    fn rejects_invalid_inputs() {
        let invalid = [
            params(Some(" "), None, None),
            params(Some("Oslo"), Some("59.9"), Some("10.7")),
            params(Some("Oslo"), Some("59.9"), None),
            params(None, Some("59.9"), None),
            params(None, None, Some("10.7")),
            params(None, Some("north"), Some("10.7")),
            params(None, Some("91"), Some("10.7")),
        ];

        for params in invalid {
            assert!(
                matches!(params.clone().parse(), Err(ApplicationError::BadRequest(_))),
                "{params:?}"
            );
        }
    }

    quickcheck! {
        fn prop_valid_coordinates_parse(lon: f32, lat: f32) -> bool {
            let expected = Coordinates::new(lon, lat);
            let parsed = params(None, Some(&lat.to_string()), Some(&lon.to_string())).parse();
            match expected {
                Ok(coordinates) => parsed == Ok(Some(LocationInput::Coordinates(coordinates))),
                Err(_) => matches!(parsed, Err(ApplicationError::BadRequest(_))),
            }
        }

        fn prop_cache_key_is_idempotent(location: String) -> bool {
            let key = location_cache_key(&location);
            location_cache_key(&key) == key
        }

        fn prop_radius_contains_its_center(lon: i16, lat: i16, radius_km: u16) -> bool {
            let center = Point::new(lon as f64 / 200.0, lat as f64 / 400.0);
            within_radius_km(center, center, radius_km as f64)
        }
    }

    #[tokio::test]
    async fn caches_lookups_case_insensitively() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/geo")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"[{"name":"Oslo","lat":59.91,"lon":10.75,"country":"NO"}]"#)
            .expect(1)
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.openweathermap_geocoding = format!("{}/geo", server.url());

        lookup_location(&state, "oslo").await.unwrap();
        let cached = lookup_location(&state, " OSLO ").await.unwrap();

        assert_eq!(cached.name, "Oslo");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn maps_empty_geocoding_result_to_not_found() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/geo")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.openweathermap_geocoding = format!("{}/geo", server.url());

        let (status, _) = make_request(
            create_test_app_with_state(state),
            "/api/nowcasts?location=Nowhere",
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_geocoding_missing_params() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/geocoding").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
