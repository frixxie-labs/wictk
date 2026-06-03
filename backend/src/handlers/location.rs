use axum::{
    extract::{Query, State},
    Json,
};
use moka::future::Cache;
use redact::Secret;
use reqwest::StatusCode;
use tracing::debug;
use wictk_core::{City, OpenWeatherMapLocation};

use crate::AppState;

use super::error::ApplicationError;

pub async fn lookup_location(
    client: &reqwest::Client,
    location: &str,
    loc_cache: &Cache<String, OpenWeatherMapLocation>,
    apikey: &Secret<String>,
) -> Result<OpenWeatherMapLocation, ApplicationError> {
    match loc_cache.get(location).await {
        Some(location) => Ok(location),
        None => {
            let locations = OpenWeatherMapLocation::fetch(client, location, apikey)
                .await
                .ok_or_else(|| {
                    tracing::error!("Failed to get location data from OpenWeatherMap");
                    ApplicationError::new(
                        "Failed to get location data from OpenWeatherMap",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
            let location = locations.first().cloned().ok_or_else(|| {
                tracing::error!("No location found for {}", location);
                ApplicationError::new("No location found", StatusCode::NOT_FOUND)
            })?;
            loc_cache
                .insert(location.name.clone(), location.clone())
                .await;
            Ok(location)
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/geocoding",
    params(City),
    responses(
        (status = 200, description = "List of matching locations", body = Vec<OpenWeatherMapLocation>),
        (status = 404, description = "No location found"),
        (status = 500, description = "Internal server error", body = String)
    ),
    tag = "geocoding"
)]
pub async fn geocoding(
    State(app_state): State<AppState>,
    Query(query): Query<City>,
) -> Result<Json<Vec<OpenWeatherMapLocation>>, ApplicationError> {
    let res = OpenWeatherMapLocation::fetch(
        &app_state.client,
        &query.location,
        &app_state.openweathermap_apikey,
    )
    .await
    .ok_or_else(|| {
        tracing::error!("Failed to get geocoding data from OpenWeatherMap");
        ApplicationError::new(
            "Failed to get geocoding data from OpenWeatherMap",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;
    debug!("Returning {:?} for {:?}", &res, &query);
    Ok(Json(res))
}

#[cfg(test)]
mod tests {
    use crate::handlers::test_utils::{create_test_app, make_request};
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_geocoding_missing_params() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/geocoding").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
