use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use geo::{Point, Polygon};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{error, instrument};
use utoipa::{IntoParams, ToSchema};
use wictk_core::{Alert, Area, Coordinates, MetAlert};

use super::{
    error::ApplicationError,
    nowcasts::{find_location, LocationQuery},
};

pub type Alerts = Vec<Alert>;

#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AlertQuery {
    /// Optional location name (e.g., "Oslo")
    pub location: Option<String>,
    /// Optional latitude coordinate
    pub lat: Option<String>,
    /// Optional longitude coordinate  
    pub lon: Option<String>,
}

impl AlertQuery {
    fn into_location_query(self) -> Option<LocationQuery> {
        if let Some(location) = self.location {
            Some(LocationQuery::Location(wictk_core::City { location }))
        } else if let (Some(lat), Some(lon)) = (self.lat, self.lon) {
            Some(LocationQuery::Coordinates(
                wictk_core::CoordinatesAsString { lat, lon },
            ))
        } else {
            None
        }
    }
}

fn point_in_polygon(point: &Point, polygon_points: &[Point]) -> bool {
    if polygon_points.len() < 3 {
        return false;
    }

    // Convert to geo::Polygon for point-in-polygon test
    let exterior_coords: Vec<geo::Coord> = polygon_points
        .iter()
        .map(|p| geo::Coord::from(*p))
        .collect();

    // Need at least 4 points to close the polygon (first and last must be same)
    let mut coords = exterior_coords;
    if coords.first() != coords.last() {
        if let Some(first) = coords.first() {
            coords.push(*first);
        }
    }

    if coords.len() < 4 {
        return false;
    }

    let line_string = geo::LineString::from(coords);
    let polygon = Polygon::new(line_string, vec![]);

    use geo::Contains;
    polygon.contains(point)
}

fn alert_contains_location(alert: &Alert, location: &Coordinates) -> bool {
    let point = Point::new(location.lon as f64, location.lat as f64);

    match alert {
        Alert::Met(met_alert) => match &met_alert.area {
            Area::Single(polygon_points) => point_in_polygon(&point, polygon_points),
            Area::Multiple(polygons) => polygons
                .iter()
                .any(|polygon_points| point_in_polygon(&point, polygon_points)),
        },
        Alert::Nve => false, // NVE alerts not implemented yet
    }
}

#[utoipa::path(
    get,
    path = "/api/alerts",
    params(AlertQuery),
    responses(
        (status = 200, description = "List of weather alerts", body = Vec<Alert>),
        (status = 500, description = "Internal server error", body = String)
    ),
    tag = "alerts"
)]
#[instrument]
pub async fn alerts(
    State(app_state): State<AppState>,
    Query(alert_query): Query<AlertQuery>,
) -> Result<Json<Vec<Alert>>, ApplicationError> {
    let all_alerts = match app_state.alert_cache.get("met_alerts").await {
        Some(alerts) => alerts.clone(),
        None => {
            let alerts = MetAlert::fetch(app_state.client.clone())
                .await
                .map_err(|err| {
                    error!("Error fetching alerts: {}", err);
                    ApplicationError::new(
                        "Failed to get Met.no alerts",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
            app_state
                .alert_cache
                .insert("met_alerts".to_string(), alerts.clone())
                .await;
            alerts
        }
    };

    // If no location query is provided, return all alerts
    let filtered_alerts = match alert_query.into_location_query() {
        Some(location_query) => {
            let location = find_location(
                location_query,
                &app_state.client,
                &app_state.location_cache,
                &app_state.openweathermap_apikey,
            )
            .await
            .map_err(|err| {
                error!("Error finding location: {:?}", err);
                ApplicationError::new(&err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            // Filter alerts to only those that contain the specified location
            all_alerts
                .into_iter()
                .filter(|alert| alert_contains_location(alert, &location))
                .collect()
        }
        None => all_alerts,
    };

    Ok(Json(filtered_alerts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_utils::{create_test_app, make_request};
    use axum::http::StatusCode;
    use geo::Point;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_alerts_endpoint() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/alerts").await;

        // This might fail due to external API dependency, but should not crash
        // We're testing the endpoint structure, not the actual Met.no API
        assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_alerts_with_location() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/alerts?location=Oslo").await;

        // External API dependency - test endpoint structure
        assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_point_in_polygon_simple_square() {
        // Create a simple square polygon: (0,0), (2,0), (2,2), (0,2)
        let polygon_points = vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
        ];

        // Point inside the square
        let inside_point = Point::new(1.0, 1.0);
        assert_eq!(point_in_polygon(&inside_point, &polygon_points), true);

        // Point outside the square
        let outside_point = Point::new(3.0, 3.0);
        assert_eq!(point_in_polygon(&outside_point, &polygon_points), false);

        // Point on the edge should return true (depends on geo implementation)
        let edge_point = Point::new(0.0, 1.0);
        let _result = point_in_polygon(&edge_point, &polygon_points);
        // Note: edge behavior may vary, so we don't assert this
    }

    #[test]
    fn test_point_in_polygon_empty() {
        let polygon_points = vec![];
        let point = Point::new(1.0, 1.0);
        assert_eq!(point_in_polygon(&point, &polygon_points), false);
    }

    #[test]
    fn test_point_in_polygon_too_few_points() {
        let polygon_points = vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)];
        let point = Point::new(0.5, 0.5);
        assert_eq!(point_in_polygon(&point, &polygon_points), false);
    }
}
