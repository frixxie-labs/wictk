use axum::{
    extract::{Query, State},
    Json,
};
use geo::Point;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};
use wictk_core::Lightning;

use crate::AppState;

use super::{
    cache::get_or_fetch,
    error::ApplicationError,
    location::{bad_request, resolve_location, within_radius_km, LocationParams},
};

const DEFAULT_RADIUS_KM: f64 = 50.0;

#[derive(Debug, Serialize, Deserialize, Default, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LightningQuery {
    /// Optional location name (e.g., "Oslo")
    pub location: Option<String>,
    /// Optional latitude coordinate
    pub lat: Option<String>,
    /// Optional longitude coordinate
    pub lon: Option<String>,
    /// Radius in kilometers to filter lightning strikes (default: 50)
    pub radius_km: Option<f64>,
}

fn strikes_within(lightning: Vec<Lightning>, center: Point, radius_km: f64) -> Vec<Lightning> {
    lightning
        .into_iter()
        .filter(|strike| within_radius_km(center, strike.location, radius_km))
        .collect()
}

#[utoipa::path(
    get,
    path = "/api/recent_lightning",
    params(LightningQuery),
    responses(
        (status = 200, description = "List of recent lightning strikes", body = Vec<Lightning>),
        (status = 400, description = "Invalid query parameters", body = String),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "Upstream provider failure", body = String)
    ),
    tag = "lightning"
)]
#[instrument(skip(app_state))]
pub async fn get_recent_lightning(
    State(app_state): State<AppState>,
    Query(query): Query<LightningQuery>,
) -> Result<Json<Vec<Lightning>>, ApplicationError> {
    let location = LocationParams {
        location: query.location,
        lat: query.lat,
        lon: query.lon,
    }
    .parse()?;
    let radius_km = query.radius_km.unwrap_or(DEFAULT_RADIUS_KM);
    if !radius_km.is_finite() || radius_km <= 0.0 {
        return Err(bad_request("'radius_km' must be a positive number"));
    }

    let lightning = get_or_fetch(
        &app_state.lightning_cache,
        (),
        Lightning::fetch(&app_state.client, &app_state.provider_urls.lightning),
        "lightning data",
    )
    .await?;

    let Some(location) = location else {
        return Ok(Json(lightning));
    };
    let center = Point::from(resolve_location(&app_state, location).await?);

    Ok(Json(strikes_within(lightning, center, radius_km)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_utils::{
        create_test_app, create_test_app_with_state, make_request, test_state,
    };
    use axum::http::StatusCode;
    use chrono::Utc;
    use quickcheck::quickcheck;

    fn strike(lon: f64, lat: f64) -> Lightning {
        Lightning::new(Point::new(lon, lat), Utc::now(), 1)
    }

    quickcheck! {
        fn prop_larger_radius_never_returns_fewer_strikes(points: Vec<(i16, i16)>, small: u16, extra: u16) -> bool {
            let strikes: Vec<Lightning> = points
                .into_iter()
                .map(|(lon, lat)| strike(lon as f64 / 200.0, lat as f64 / 400.0))
                .collect();
            let center = Point::new(10.0, 60.0);
            let small = small as f64;
            let large = small + extra as f64;

            strikes_within(strikes.clone(), center, small).len()
                <= strikes_within(strikes, center, large).len()
        }
    }

    #[test]
    fn keeps_only_strikes_inside_radius() {
        let oslo = strike(10.7522, 59.9139);
        let trondheim = strike(10.4034, 63.4308);

        let kept = strikes_within(vec![oslo, trondheim], Point::new(10.75, 59.91), 50.0);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].location, Point::new(10.7522, 59.9139));
    }

    #[tokio::test]
    async fn filters_cached_lightning_by_coordinates() {
        let state = test_state();
        state
            .lightning_cache
            .insert((), vec![strike(10.7522, 59.9139), strike(10.4034, 63.4308)])
            .await;
        let app = create_test_app_with_state(state);

        let (status, body) = make_request(
            app,
            "/api/recent_lightning?lat=59.91&lon=10.75&radius_km=25",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let strikes: Vec<Lightning> = serde_json::from_slice(&body).unwrap();
        assert_eq!(strikes.len(), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_queries() {
        for query in [
            "/api/recent_lightning?radius_km=-1",
            "/api/recent_lightning?lat=59.9",
            "/api/recent_lightning?lat=abc&lon=10",
        ] {
            let (status, _) = make_request(create_test_app(), query).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        }
    }
}
