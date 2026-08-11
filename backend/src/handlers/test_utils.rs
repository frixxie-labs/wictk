use crate::{handlers::setup_router, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tower::ServiceExt;
use wictk_core::USGS_ALL_DAY_FEED_URL;

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

pub fn create_test_app() -> axum::Router {
    let client = reqwest::Client::new();
    let app_state = AppState::new(
        client,
        "test_api_key".to_string(),
        USGS_ALL_DAY_FEED_URL.to_string(),
    );
    create_test_app_with_state(app_state)
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
