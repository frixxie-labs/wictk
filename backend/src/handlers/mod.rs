use crate::AppState;
use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use lightning::get_recent_lightning;
use metrics::histogram;
use metrics_exporter_prometheus::PrometheusHandle;
use nowcasts::{nowcast_met, nowcast_openweathermap, nowcasts};
use tokio::time::Instant;
use tower::ServiceBuilder;
use tracing::{info, instrument};
use utoipa::OpenApi;
use wictk_core::{
    Alert, Area, City, Coordinates, CoordinatesAsString, Lightning, MetAlert, MetNowcast, Nowcast,
    OpenWeatherMapLocation, OpenWeatherNowcast, Severity, TimeDuration,
};

use self::{
    alerts::alerts,
    location::geocoding,
    status::{health, ping},
};

mod alerts;
mod error;
mod lightning;
mod location;
mod nowcasts;
mod status;

#[cfg(test)]
mod test_utils;

pub use alerts::Alerts;

#[derive(OpenApi)]
#[openapi(
    paths(
        status::ping,
        status::health,
        alerts::alerts,
        nowcasts::nowcast_met,
        nowcasts::nowcast_openweathermap,
        nowcasts::nowcasts,
        location::geocoding,
        lightning::get_recent_lightning,
        openapi,
    ),
    components(
        schemas(
            Nowcast,
            MetNowcast,
            OpenWeatherNowcast,
            Alert,
            MetAlert,
            Severity,
            Area,
            TimeDuration,
            Lightning,
            Coordinates,
            CoordinatesAsString,
            City,
            OpenWeatherMapLocation,
            nowcasts::LocationQuery,
            nowcasts::LocationParams,
            alerts::AlertQuery,
            lightning::LightningQuery,
        )
    ),
    tags(
        (name = "status", description = "Health check endpoints"),
        (name = "nowcasts", description = "Weather nowcast endpoints"),
        (name = "alerts", description = "Weather alert endpoints"),
        (name = "geocoding", description = "Geocoding endpoints"),
        (name = "lightning", description = "Lightning data endpoints"),
        (name = "documentation", description = "API documentation endpoints"),
    ),
    info(
        title = "WICTK Weather API",
        description = "Weather Information and Climate Toolkit API",
        version = "0.20.1"
    )
)]
pub struct ApiDoc;

#[instrument]
pub async fn profile_endpoint(request: Request, next: Next) -> Response {
    let method = request.method().clone().to_string();
    let uri = request.uri().clone().to_string();

    info!("Handling {} at {}", method, uri);

    let now = Instant::now();

    let response = next.run(request).await;

    let elapsed = now.elapsed();

    let labels = [("method", method.clone()), ("uri", uri.clone())];

    histogram!("handler", &labels).record(elapsed);

    info!(
        "Finished handling {} at {}, used {} ms",
        method,
        uri,
        elapsed.as_millis()
    );
    response
}

pub fn setup_router(app_state: AppState, metrics_handler: PrometheusHandle) -> Router {
    let api = Router::new()
        .route("/alerts", get(alerts))
        .route("/owm/nowcasts", get(nowcast_openweathermap))
        .route("/met/nowcasts", get(nowcast_met))
        .route("/nowcasts", get(nowcasts))
        .route("/geocoding", get(geocoding))
        .route("/recent_lightning", get(get_recent_lightning))
        .with_state(app_state);

    let status = Router::new()
        .route("/ping", get(ping))
        .route("/health", get(health));

    Router::new()
        .route("/metrics", get(metrics))
        .route("/openapi", get(openapi))
        .with_state(metrics_handler)
        .nest("/status", status)
        .nest("/api", api)
        .layer(ServiceBuilder::new().layer(middleware::from_fn(profile_endpoint)))
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    responses(
        (status = 200, description = "OpenAPI specification")
    ),
    tag = "documentation"
)]
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[instrument]
async fn metrics(State(handle): State<PrometheusHandle>) -> String {
    handle.render()
}

#[cfg(test)]
mod tests {
    use crate::handlers::test_utils::{create_test_app, make_request, make_request_with_method};
    use axum::http::StatusCode;
    use axum::{extract::Query, http::Uri};
    use wictk_core::{City, CoordinatesAsString};

    use crate::handlers::nowcasts::LocationQuery;

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/metrics").await;

        assert_eq!(status, StatusCode::OK);

        // Metrics endpoint may return empty if no metrics have been recorded yet
        // The important thing is that it responds with 200 OK
    }

    #[tokio::test]
    async fn test_invalid_endpoint() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/invalid").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_invalid_method() {
        let app = create_test_app();
        let (status, _body) = make_request_with_method(app, "POST", "/status/ping").await;

        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_endpoint_timing_metrics() {
        let app = create_test_app();

        // Make a request to generate metrics
        let _ = make_request(app.clone(), "/status/ping").await;

        // Check that metrics are generated
        let (status, body) = make_request(app, "/metrics").await;
        assert_eq!(status, StatusCode::OK);

        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("handler"));
    }

    #[test]
    fn parse_location() {
        let uri: Uri = "http://localhost:3000/api/nowcasts?location=Oslo"
            .parse()
            .unwrap();

        let query = Query::<LocationQuery>::try_from_uri(&uri).unwrap();

        assert_eq!(
            query.0,
            LocationQuery::Location(City {
                location: "Oslo".to_string()
            })
        );
    }

    #[test]
    fn parse_coordinates() {
        let uri: Uri = "http://localhost:3000/api/nowcasts?lat=59.91273&lon=10.74609"
            .parse()
            .unwrap();

        let query = Query::<LocationQuery>::try_from_uri(&uri).unwrap();

        assert_eq!(
            query.0,
            LocationQuery::Coordinates(CoordinatesAsString {
                lat: "59.91273".to_string(),
                lon: "10.74609".to_string()
            })
        );
    }

    #[test]
    fn test_locationtype_city() {
        let json = r#"{"location": "Oslo"}"#;
        let location: LocationQuery = serde_json::from_str(json).unwrap();
        assert_eq!(
            location,
            LocationQuery::Location(City {
                location: "Oslo".to_string()
            })
        );
    }

    #[test]
    fn test_locationtype_coordinates_strings() {
        let json = r#"{"lat": "1.0", "lon": "2.0"}"#;
        let location: LocationQuery = serde_json::from_str(json).unwrap();
        assert_eq!(
            location,
            LocationQuery::Coordinates(CoordinatesAsString {
                lat: "1.0".to_string(),
                lon: "2.0".to_string()
            })
        );
    }
}
