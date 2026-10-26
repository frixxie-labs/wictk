use axum::{
    extract::{Query, State},
    Json,
};
use geo::Point;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};
use wictk_core::Earthquake;

use crate::AppState;

use super::{
    cache::get_or_fetch,
    error::ApplicationError,
    location::{bad_request, resolve_location, within_radius_km, LocationInput, LocationParams},
};

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
    pub lat: Option<String>,
    /// Optional longitude coordinate
    pub lon: Option<String>,
    /// Radius in kilometers; defaults to 50 when a location is provided (maximum: 20001.6)
    pub radius_km: Option<f64>,
    /// Minimum earthquake magnitude
    pub min_magnitude: Option<f64>,
    /// Maximum number of events to return (default: 100, maximum: 500)
    pub limit: Option<usize>,
}

struct ValidatedEarthquakeQuery {
    area: Option<(LocationInput, f64)>,
    criteria: Criteria,
}

/// Pure selection criteria applied to the feed.
#[derive(Debug, Clone, PartialEq)]
struct Criteria {
    min_magnitude: Option<f64>,
    limit: usize,
}

impl TryFrom<EarthquakeQuery> for ValidatedEarthquakeQuery {
    type Error = ApplicationError;

    fn try_from(query: EarthquakeQuery) -> Result<Self, Self::Error> {
        let location = LocationParams {
            location: query.location,
            lat: query.lat,
            lon: query.lon,
        }
        .parse()?;

        if query
            .radius_km
            .is_some_and(|radius| !radius.is_finite() || radius <= 0.0 || radius > MAX_RADIUS_KM)
        {
            return Err(bad_request(
                "'radius_km' must be greater than 0 and no more than 20001.6",
            ));
        }
        let area = match (location, query.radius_km) {
            (Some(location), radius_km) => Some((location, radius_km.unwrap_or(DEFAULT_RADIUS_KM))),
            (None, Some(_)) => {
                return Err(bad_request(
                    "'radius_km' requires 'location' or 'lat' and 'lon'",
                ))
            }
            (None, None) => None,
        };

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

        Ok(Self {
            area,
            criteria: Criteria {
                min_magnitude: query.min_magnitude,
                limit,
            },
        })
    }
}

/// Filters by magnitude and area, orders newest first and applies the limit.
fn select_earthquakes(
    earthquakes: Vec<Earthquake>,
    criteria: &Criteria,
    area: Option<(Point, f64)>,
) -> Vec<Earthquake> {
    let mut selected: Vec<Earthquake> = earthquakes
        .into_iter()
        .filter(|earthquake| {
            criteria.min_magnitude.is_none_or(|minimum| {
                earthquake
                    .magnitude
                    .is_some_and(|magnitude| magnitude >= minimum)
            })
        })
        .filter(|earthquake| {
            area.is_none_or(|(center, radius_km)| {
                within_radius_km(center, earthquake.coordinates.into(), radius_km)
            })
        })
        .collect();
    selected.sort_by(|left, right| {
        right
            .time
            .cmp(&left.time)
            .then_with(|| left.id.cmp(&right.id))
    });
    selected.truncate(criteria.limit);
    selected
}

#[utoipa::path(
    get,
    path = "/api/earthquakes",
    params(EarthquakeQuery),
    responses(
        (status = 200, description = "Recent earthquakes, newest first", body = Vec<Earthquake>),
        (status = 400, description = "Invalid query parameters", body = String),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "USGS feed or geocoding provider unavailable", body = String)
    ),
    tag = "earthquakes"
)]
#[instrument(skip(app_state))]
pub async fn get_earthquakes(
    State(app_state): State<AppState>,
    Query(query): Query<EarthquakeQuery>,
) -> Result<Json<Vec<Earthquake>>, ApplicationError> {
    let query = ValidatedEarthquakeQuery::try_from(query)?;

    let area = match query.area {
        Some((location, radius_km)) => Some((
            Point::from(resolve_location(&app_state, location).await?),
            radius_km,
        )),
        None => None,
    };

    let earthquakes = get_or_fetch(
        &app_state.earthquake_cache,
        (),
        Earthquake::fetch_feed(&app_state.client, &app_state.provider_urls.earthquakes),
        "USGS earthquake feed",
    )
    .await?;

    Ok(Json(select_earthquakes(earthquakes, &query.criteria, area)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::{
        setup_router,
        test_utils::{create_test_app_with_state, get_metrics_handle, make_request},
    };
    use axum::http::StatusCode;
    use chrono::{DateTime, Utc};
    use quickcheck::quickcheck;
    use serde_json::json;
    use wictk_core::{Coordinates, EarthquakeAlertLevel, EarthquakeStatus, OpenWeatherMapLocation};

    fn earthquake(id: &str, magnitude: Option<f64>, time: i64, lon: f32, lat: f32) -> Earthquake {
        Earthquake {
            id: id.to_string(),
            magnitude,
            place: Some(format!("Place {id}")),
            time: DateTime::<Utc>::from_timestamp(time, 0).unwrap(),
            updated: DateTime::<Utc>::from_timestamp(time + 30, 0).unwrap(),
            coordinates: Coordinates::new(lon, lat).unwrap(),
            depth_km: 8.0,
            significance: Some(100),
            alert_level: Some(EarthquakeAlertLevel::Green),
            status: EarthquakeStatus::Reviewed,
            tsunami: false,
            details_url: format!("https://example.com/{id}"),
        }
    }

    fn test_state(feed_url: &str) -> AppState {
        let mut state = crate::handlers::test_utils::test_state();
        state.provider_urls.earthquakes = feed_url.to_string();
        state
    }

    quickcheck! {
        fn prop_selection_is_sorted_limited_and_meets_minimum(
            events: Vec<(Option<u8>, u32)>,
            min_magnitude: Option<u8>,
            limit: u8
        ) -> bool {
            let earthquakes: Vec<Earthquake> = events
                .iter()
                .enumerate()
                .map(|(i, (magnitude, time))| {
                    earthquake(&i.to_string(), magnitude.map(f64::from), *time as i64, 10.0, 60.0)
                })
                .collect();
            let criteria = Criteria {
                min_magnitude: min_magnitude.map(f64::from),
                limit: limit as usize + 1,
            };

            let selected = select_earthquakes(earthquakes, &criteria, None);

            selected.len() <= criteria.limit
                && selected.windows(2).all(|pair| pair[0].time >= pair[1].time)
                && selected.iter().all(|earthquake| {
                    criteria
                        .min_magnitude
                        .is_none_or(|minimum| earthquake.magnitude.is_some_and(|m| m >= minimum))
                })
        }
    }

    async fn app_with_earthquakes(earthquakes: Vec<Earthquake>) -> axum::Router {
        let state = crate::handlers::test_utils::test_state();
        state.earthquake_cache.insert((), earthquakes).await;
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
        let state = crate::handlers::test_utils::test_state();
        state
            .earthquake_cache
            .insert(
                (),
                vec![
                    earthquake("oslo", Some(2.0), 200, 10.7522, 59.9139),
                    earthquake("trondheim", Some(2.0), 100, 10.4034, 63.4308),
                ],
            )
            .await;
        state
            .location_cache
            .insert(
                "oslo".to_string(),
                OpenWeatherMapLocation {
                    name: "Oslo".to_string(),
                    local_names: None,
                    location: Coordinates::new(10.7522, 59.9139).unwrap(),
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
