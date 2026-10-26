use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use geo::{Contains, LineString, Point, Polygon};
use tracing::instrument;
use wictk_core::{Alert, Area, MetAlert};

use super::{
    cache::get_or_fetch,
    error::ApplicationError,
    location::{resolve_location, LocationParams},
};

pub type Alerts = Vec<Alert>;

fn point_in_polygon(point: &Point, polygon_points: &[Point]) -> bool {
    if polygon_points.len() < 3 {
        return false;
    }
    // `Polygon::new` closes the ring if needed.
    let exterior: LineString = polygon_points.iter().copied().collect();
    Polygon::new(exterior, vec![]).contains(point)
}

fn alert_contains_point(alert: &Alert, point: &Point) -> bool {
    let Alert::Met(met_alert) = alert;
    match &met_alert.area {
        Area::Single(polygon) => point_in_polygon(point, polygon),
        Area::Multiple(polygons) => polygons
            .iter()
            .any(|polygon| point_in_polygon(point, polygon)),
    }
}

#[utoipa::path(
    get,
    path = "/api/alerts",
    params(LocationParams),
    responses(
        (status = 200, description = "List of weather alerts", body = Vec<Alert>),
        (status = 400, description = "Invalid location parameters", body = String),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "Upstream provider failure", body = String)
    ),
    tag = "alerts"
)]
#[instrument(skip(app_state))]
pub async fn alerts(
    State(app_state): State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Alerts>, ApplicationError> {
    let location = params.parse()?;
    let all_alerts = get_or_fetch(
        &app_state.alert_cache,
        (),
        MetAlert::fetch(&app_state.client, &app_state.provider_urls.met_alerts),
        "Met.no alerts",
    )
    .await?;

    let Some(location) = location else {
        return Ok(Json(all_alerts));
    };
    let point = Point::from(resolve_location(&app_state, location).await?);

    Ok(Json(
        all_alerts
            .into_iter()
            .filter(|alert| alert_contains_point(alert, &point))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_utils::{create_test_app_with_state, make_request, test_state};
    use axum::http::StatusCode;
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use quickcheck::{quickcheck, TestResult};
    use wictk_core::{AlertEvent, Certainty, Severity, TimeDuration};

    const ALERTS_FIXTURE: &str =
        include_str!("../../../wictk_core/tests/fixtures/met_alerts_multipolygon.json");

    fn square(x: f64, y: f64, size: f64) -> Vec<Point> {
        vec![
            Point::new(x, y),
            Point::new(x + size, y),
            Point::new(x + size, y + size),
            Point::new(x, y + size),
        ]
    }

    fn alert(area: Area) -> Alert {
        Alert::Met(MetAlert {
            title: "title".to_string(),
            severity: Severity::Yellow,
            description: "description".to_string(),
            certainty: Certainty::Likely,
            event: AlertEvent::Gale,
            duration: TimeDuration {
                from: Utc::now(),
                until: Utc::now(),
            },
            area,
        })
    }

    quickcheck! {
        fn prop_square_contains_its_center(x: i16, y: i16, size: u8) -> TestResult {
            if size == 0 {
                return TestResult::discard();
            }
            let (x, y, size) = (x as f64, y as f64, size as f64);
            let center = Point::new(x + size / 2.0, y + size / 2.0);
            let outside = Point::new(x + size * 2.0, y + size * 2.0);
            let polygon = square(x, y, size);

            TestResult::from_bool(
                point_in_polygon(&center, &polygon) && !point_in_polygon(&outside, &polygon),
            )
        }

        fn prop_ring_closure_does_not_change_containment(px: i8, py: i8) -> bool {
            let point = Point::new(px as f64 / 10.0, py as f64 / 10.0);
            let open = square(0.0, 0.0, 2.0);
            let mut closed = open.clone();
            closed.push(open[0]);

            point_in_polygon(&point, &open) == point_in_polygon(&point, &closed)
        }
    }

    #[test]
    fn degenerate_polygons_contain_nothing() {
        let point = Point::new(0.5, 0.5);
        assert!(!point_in_polygon(&point, &[]));
        assert!(!point_in_polygon(
            &point,
            &[Point::new(0.0, 0.0), Point::new(1.0, 1.0)]
        ));
    }

    #[test]
    fn multi_polygon_alert_matches_any_polygon() {
        let alert = alert(Area::Multiple(vec![
            square(0.0, 0.0, 1.0),
            square(10.0, 10.0, 1.0),
        ]));

        assert!(alert_contains_point(&alert, &Point::new(10.5, 10.5)));
        assert!(!alert_contains_point(&alert, &Point::new(5.0, 5.0)));
    }

    #[tokio::test]
    async fn filters_alerts_by_coordinates() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/alerts")
            .with_status(200)
            .with_body(ALERTS_FIXTURE)
            .expect(1)
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.met_alerts = format!("{}/alerts", server.url());
        let app = create_test_app_with_state(state);

        let (all_status, all_body) = make_request(app.clone(), "/api/alerts").await;
        let (status, body) = make_request(app, "/api/alerts?lat=62.1&lon=5.0").await;

        assert_eq!(all_status, StatusCode::OK);
        assert_eq!(status, StatusCode::OK);
        let all: Alerts = serde_json::from_slice(&all_body).unwrap();
        let filtered: Alerts = serde_json::from_slice(&body).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(filtered.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn maps_provider_failure_to_bad_gateway() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/alerts")
            .with_status(503)
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.met_alerts = format!("{}/alerts", server.url());

        let (status, _) = make_request(create_test_app_with_state(state), "/api/alerts").await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }
}
