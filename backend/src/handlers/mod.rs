use crate::AppState;
use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use earthquakes::get_earthquakes;
use lightning::get_recent_lightning;
use metrics::histogram;
use metrics_exporter_prometheus::PrometheusHandle;
use nowcasts::{nowcast_met, nowcast_openweathermap, nowcasts};
use tokio::time::Instant;
use tower::ServiceBuilder;
use tracing::{info, instrument};
use utoipa::OpenApi;
use wictk_core::{
    Alert, AlertEvent, Area, Certainty, City, Coordinates, Earthquake, EarthquakeAlertLevel,
    EarthquakeStatus, Lightning, MetAlert, MetNowcast, Nowcast, OpenWeatherMapLocation,
    OpenWeatherNowcast, Severity, TimeDuration,
};

use self::{
    alerts::alerts,
    location::geocoding,
    status::{health, ping},
};

mod alerts;
mod cache;
mod earthquakes;
mod error;
mod lightning;
mod location;
mod nowcasts;
mod status;

#[cfg(test)]
mod test_utils;

pub use alerts::Alerts;
pub use nowcasts::NowcastKey;

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
        earthquakes::get_earthquakes,
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
            Certainty,
            AlertEvent,
            Area,
            TimeDuration,
            Lightning,
            Earthquake,
            EarthquakeAlertLevel,
            EarthquakeStatus,
            Coordinates,
            City,
            OpenWeatherMapLocation,
            location::LocationParams,
            lightning::LightningQuery,
            earthquakes::EarthquakeQuery,
        )
    ),
    tags(
        (name = "status", description = "Health check endpoints"),
        (name = "nowcasts", description = "Weather nowcast endpoints"),
        (name = "alerts", description = "Weather alert endpoints"),
        (name = "geocoding", description = "Geocoding endpoints"),
        (name = "lightning", description = "Lightning data endpoints"),
        (name = "earthquakes", description = "Recent earthquake endpoints"),
        (name = "documentation", description = "API documentation endpoints"),
    ),
    info(
        title = "WICTK API",
        description = "Weather and environmental event API",
        version = "0.20.1"
    )
)]
pub struct ApiDoc;

#[instrument]
pub async fn profile_endpoint(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().to_string();

    info!("Handling {} at {}", method, uri);

    let now = Instant::now();

    let response = next.run(request).await;

    let elapsed = now.elapsed();

    histogram!("handler", "method" => method.clone(), "uri" => uri.clone()).record(elapsed);

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
        .route("/earthquakes", get(get_earthquakes))
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
    use quickcheck::quickcheck;

    use crate::handlers::location::LocationParams;

    fn safe_query_value(value: String) -> String {
        value
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
            .collect()
    }

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

    quickcheck! {
        fn prop_location_params_parse_from_query(location: String, lat: String, lon: String) -> bool {
            let (location, lat, lon) = (safe_query_value(location), safe_query_value(lat), safe_query_value(lon));
            let uri: Uri = format!("http://localhost:3000/api/nowcasts?location={location}&lat={lat}&lon={lon}")
                .parse()
                .unwrap();

            let query = Query::<LocationParams>::try_from_uri(&uri).unwrap();

            query.0 == LocationParams { location: Some(location), lat: Some(lat), lon: Some(lon) }
        }
    }
}
