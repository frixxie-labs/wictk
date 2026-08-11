use axum::{
    extract::{Query, State},
    Json,
};
use geo::{Distance, Haversine, Point};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{error, instrument};
use utoipa::{IntoParams, ToSchema};
use wictk_core::{Coordinates, Earthquake};

use crate::AppState;

use super::{error::ApplicationError, location::lookup_location};

const EARTHQUAKE_CACHE_KEY: &str = "recent_earthquakes";
const DEFAULT_RADIUS_KM: f64 = 50.0;
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const MAX_RADIUS_KM: f64 = 20_001.6;

#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EarthquakeQuery {
    /// Optional location name (e.g., "Oslo")
    pub location: Option<String>,
    /// Optional latitude coordinate
    pub lat: Option<f64>,
    /// Optional longitude coordinate
    pub lon: Option<f64>,
    /// Radius in kilometers; defaults to 50 when a location is provided (maximum: 20001.6)
    pub radius_km: Option<f64>,
    /// Minimum earthquake magnitude
    pub min_magnitude: Option<f64>,
    /// Maximum number of events to return (default: 100, maximum: 500)
    pub limit: Option<usize>,
}

enum EarthquakeLocation {
    City(String),
    Coordinates(Coordinates),
}

struct ValidatedEarthquakeQuery {
    location: Option<EarthquakeLocation>,
    radius_km: Option<f64>,
    min_magnitude: Option<f64>,
    limit: usize,
}

impl TryFrom<EarthquakeQuery> for ValidatedEarthquakeQuery {
    type Error = ApplicationError;

    fn try_from(query: EarthquakeQuery) -> Result<Self, Self::Error> {
        if query.location.is_some() && (query.lat.is_some() || query.lon.is_some()) {
            return Err(bad_request(
                "Provide either 'location' or 'lat' and 'lon', not both",
            ));
        }

        let location = match (query.location, query.lat, query.lon) {
            (Some(location), None, None) => {
                let location = location.trim();
                if location.is_empty() {
                    return Err(bad_request("'location' must not be empty"));
                }
                Some(EarthquakeLocation::City(location.to_string()))
            }
            (None, Some(lat), Some(lon)) => {
                if !lat.is_finite()
                    || !lon.is_finite()
                    || !(-90.0..=90.0).contains(&lat)
                    || !(-180.0..=180.0).contains(&lon)
                {
                    return Err(bad_request("Latitude or longitude is out of range"));
                }
                Some(EarthquakeLocation::Coordinates(Coordinates::new(
                    lon as f32, lat as f32,
                )))
            }
            (None, None, None) => None,
            (None, _, _) => {
                return Err(bad_request("Provide both 'lat' and 'lon'"));
            }
            (Some(_), _, _) => unreachable!("mixed location inputs were rejected above"),
        };

        if location.is_none() && query.radius_km.is_some() {
            return Err(bad_request(
                "'radius_km' requires 'location' or 'lat' and 'lon'",
            ));
        }
        if query
            .radius_km
            .is_some_and(|radius| !radius.is_finite() || radius <= 0.0 || radius > MAX_RADIUS_KM)
        {
            return Err(bad_request(
                "'radius_km' must be greater than 0 and no more than 20001.6",
            ));
        }
        if query
            .min_magnitude
            .is_some_and(|magnitude| !magnitude.is_finite())
        {
            return Err(bad_request("'min_magnitude' must be a finite number"));
        }

        let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(bad_request("'limit' must be between 1 and 500"));
        }

        let radius_km = location
            .as_ref()
            .map(|_| query.radius_km.unwrap_or(DEFAULT_RADIUS_KM));

        Ok(Self {
            location,
            radius_km,
            min_magnitude: query.min_magnitude,
            limit,
        })
    }
}

fn bad_request(message: &str) -> ApplicationError {
    ApplicationError::new(message, StatusCode::BAD_REQUEST)
}

#[utoipa::path(
    get,
    path = "/api/earthquakes",
    params(EarthquakeQuery),
    responses(
        (status = 200, description = "Recent earthquakes, newest first", body = Vec<Earthquake>),
        (status = 400, description = "Invalid query parameters", body = String),
        (status = 404, description = "Location not found", body = String),
        (status = 500, description = "Geocoding provider failure", body = String),
        (status = 502, description = "USGS earthquake feed unavailable", body = String)
    ),
    tag = "earthquakes"
)]
#[instrument(skip(app_state))]
pub async fn get_earthquakes(
    State(app_state): State<AppState>,
    Query(query): Query<EarthquakeQuery>,
) -> Result<Json<Vec<Earthquake>>, ApplicationError> {
    let query = ValidatedEarthquakeQuery::try_from(query)?;

    let center = match query.location {
        Some(EarthquakeLocation::City(location)) => Some(
            lookup_location(
                &app_state.client,
                &location,
                &app_state.location_cache,
                &app_state.openweathermap_apikey,
            )
            .await?
            .location,
        ),
        Some(EarthquakeLocation::Coordinates(coordinates)) => Some(coordinates),
        None => None,
    };

    let mut earthquakes = app_state
        .earthquake_cache
        .try_get_with(
            EARTHQUAKE_CACHE_KEY.to_string(),
            Earthquake::fetch_feed(&app_state.client, &app_state.earthquake_feed_url),
        )
        .await
        .map_err(|err| {
            error!(error = ?err, "Failed to fetch USGS earthquake feed");
            ApplicationError::new(
                "Failed to fetch USGS earthquake feed",
                StatusCode::BAD_GATEWAY,
            )
        })?;

    earthquakes.retain(|earthquake| {
        if query
            .min_magnitude
            .is_some_and(|minimum| earthquake.magnitude.is_none_or(|value| value < minimum))
        {
            return false;
        }

        match (&center, query.radius_km) {
            (Some(center), Some(radius_km)) => {
                let center = Point::new(center.lon as f64, center.lat as f64);
                let event = Point::new(
                    earthquake.coordinates.lon as f64,
                    earthquake.coordinates.lat as f64,
                );
                Haversine.distance(center, event) <= radius_km * 1000.0
            }
            _ => true,
        }
    });
    earthquakes.sort_by(|left, right| {
        right
            .time
            .cmp(&left.time)
            .then_with(|| left.id.cmp(&right.id))
    });
    earthquakes.truncate(query.limit);

    Ok(Json(earthquakes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::{
        setup_router,
        test_utils::{create_test_app_with_state, get_metrics_handle, make_request},
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use wictk_core::{
        EarthquakeAlertLevel, EarthquakeStatus, OpenWeatherMapLocation, USGS_ALL_DAY_FEED_URL,
    };

    fn earthquake(id: &str, magnitude: Option<f64>, time: i64, lon: f32, lat: f32) -> Earthquake {
        Earthquake {
            id: id.to_string(),
            magnitude,
            place: Some(format!("Place {id}")),
            time: DateTime::<Utc>::from_timestamp(time, 0).unwrap(),
            updated: DateTime::<Utc>::from_timestamp(time + 30, 0).unwrap(),
            coordinates: Coordinates::new(lon, lat),
            depth_km: 8.0,
            significance: Some(100),
            alert_level: Some(EarthquakeAlertLevel::Green),
            status: EarthquakeStatus::Reviewed,
            tsunami: false,
            details_url: format!("https://example.com/{id}"),
        }
    }

    fn test_state(feed_url: &str) -> AppState {
        AppState::new(
            reqwest::Client::new(),
            "test_api_key".to_string(),
            feed_url.to_string(),
        )
    }

    async fn app_with_earthquakes(earthquakes: Vec<Earthquake>) -> axum::Router {
        let state = test_state(USGS_ALL_DAY_FEED_URL);
        state
            .earthquake_cache
            .insert(EARTHQUAKE_CACHE_KEY.to_string(), earthquakes)
            .await;
        create_test_app_with_state(state)
    }

    #[tokio::test]
    async fn filters_magnitude_orders_newest_first_and_limits_results() {
        let app = app_with_earthquakes(vec![
            earthquake("older", Some(5.0), 100, 10.0, 60.0),
            earthquake("no-magnitude", None, 300, 10.0, 60.0),
            earthquake("newer", Some(3.0), 200, 10.0, 60.0),
        ])
        .await;

        let (status, body) = make_request(app, "/api/earthquakes?min_magnitude=2.0&limit=1").await;

        assert_eq!(status, StatusCode::OK);
        let earthquakes: Vec<Earthquake> = serde_json::from_slice(&body).unwrap();
        assert_eq!(earthquakes.len(), 1);
        assert_eq!(earthquakes[0].id, "newer");
    }

    #[tokio::test]
    async fn filters_by_coordinate_radius() {
        let app = app_with_earthquakes(vec![
            earthquake("oslo", Some(2.0), 200, 10.7522, 59.9139),
            earthquake("trondheim", Some(2.0), 100, 10.4034, 63.4308),
        ])
        .await;

        let (status, body) =
            make_request(app, "/api/earthquakes?lat=59.9139&lon=10.7522&radius_km=25").await;

        assert_eq!(status, StatusCode::OK);
        let earthquakes: Vec<Earthquake> = serde_json::from_slice(&body).unwrap();
        assert_eq!(earthquakes.len(), 1);
        assert_eq!(earthquakes[0].id, "oslo");
    }

    #[tokio::test]
    async fn resolves_cached_city_for_radius_filter() {
        let state = test_state(USGS_ALL_DAY_FEED_URL);
        state
            .earthquake_cache
            .insert(
                EARTHQUAKE_CACHE_KEY.to_string(),
                vec![
                    earthquake("oslo", Some(2.0), 200, 10.7522, 59.9139),
                    earthquake("trondheim", Some(2.0), 100, 10.4034, 63.4308),
                ],
            )
            .await;
        state
            .location_cache
            .insert(
                "Oslo".to_string(),
                OpenWeatherMapLocation {
                    name: "Oslo".to_string(),
                    local_names: None,
                    location: Coordinates::new(10.7522, 59.9139),
                    country: "NO".to_string(),
                    state: None,
                },
            )
            .await;
        let app = create_test_app_with_state(state);

        let (status, body) = make_request(app, "/api/earthquakes?location=Oslo&radius_km=25").await;

        assert_eq!(status, StatusCode::OK);
        let earthquakes: Vec<Earthquake> = serde_json::from_slice(&body).unwrap();
        assert_eq!(earthquakes.len(), 1);
        assert_eq!(earthquakes[0].id, "oslo");
    }

    #[tokio::test]
    async fn rejects_invalid_queries() {
        let app = app_with_earthquakes(Vec::new()).await;
        let invalid_queries = [
            "/api/earthquakes?lat=59.9",
            "/api/earthquakes?lat=91&lon=10",
            "/api/earthquakes?location=Oslo&lat=59.9&lon=10.7",
            "/api/earthquakes?radius_km=25",
            "/api/earthquakes?location=Oslo&radius_km=0",
            "/api/earthquakes?location=Oslo&radius_km=20002",
            "/api/earthquakes?min_magnitude=NaN",
            "/api/earthquakes?limit=0",
            "/api/earthquakes?limit=501",
            "/api/earthquakes?location=%20",
        ];

        for query in invalid_queries {
            let (status, _body) = make_request(app.clone(), query).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "query: {query}");
        }
    }

    #[tokio::test]
    async fn fetches_and_caches_the_feed() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/all_day.geojson")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "properties": {
                            "mag": 2.1,
                            "place": "Test place",
                            "time": 1_721_300_000_000_i64,
                            "updated": 1_721_300_060_000_i64,
                            "url": "https://example.com/test",
                            "sig": 70,
                            "alert": null,
                            "status": "automatic",
                            "tsunami": 0,
                            "type": "earthquake"
                        },
                        "geometry": {
                            "type": "Point",
                            "coordinates": [10.2, 59.1, 8.3]
                        },
                        "id": "test"
                    }]
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let state = test_state(&format!("{}/all_day.geojson", server.url()));
        let app = setup_router(state, get_metrics_handle());

        let (first_status, _) = make_request(app.clone(), "/api/earthquakes").await;
        let (second_status, _) = make_request(app, "/api/earthquakes").await;

        assert_eq!(first_status, StatusCode::OK);
        assert_eq!(second_status, StatusCode::OK);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn maps_feed_failures_to_bad_gateway() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/all_day.geojson")
            .with_status(503)
            .create_async()
            .await;
        let state = test_state(&format!("{}/all_day.geojson", server.url()));
        let app = create_test_app_with_state(state);

        let (status, _body) = make_request(app, "/api/earthquakes").await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }
}
