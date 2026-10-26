use crate::{handlers::setup_router, AppState, ProviderUrls};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tower::ServiceExt;

static METRICS_HANDLE: Lazy<Mutex<Option<PrometheusHandle>>> = Lazy::new(|| Mutex::new(None));

pub fn get_metrics_handle() -> PrometheusHandle {
    let mut handle = METRICS_HANDLE.lock().unwrap();
    if handle.is_none() {
        let prometheus_handle = PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install recorder/exporter");
        *handle = Some(prometheus_handle);
    }
    handle.as_ref().unwrap().clone()
}

/// State whose providers point at an unroutable address, so tests never reach real APIs.
pub fn test_state() -> AppState {
    let unreachable = "http://127.0.0.1:9".to_string();
    AppState::new(
        reqwest::Client::new(),
        "test_api_key".to_string(),
        ProviderUrls {
            met_alerts: unreachable.clone(),
            met_nowcast: unreachable.clone(),
            openweathermap_nowcast: unreachable.clone(),
            openweathermap_geocoding: unreachable.clone(),
            lightning: unreachable.clone(),
            earthquakes: unreachable,
        },
    )
}

pub fn create_test_app() -> axum::Router {
    create_test_app_with_state(test_state())
}

pub fn create_test_app_with_state(app_state: AppState) -> axum::Router {
    setup_router(app_state, get_metrics_handle())
}

pub async fn make_request(app: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    (status, body.to_vec())
}

pub async fn make_request_with_method(
    app: axum::Router,
    method: &str,
    uri: &str,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    (status, body.to_vec())
}
