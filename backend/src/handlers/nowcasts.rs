use axum::{
    extract::{Query, State},
    Json,
};
use moka::future::Cache;
use redact::Secret;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::error;
use tracing::instrument;
use utoipa::{IntoParams, ToSchema};
use wictk_core::{
    City, Coordinates, CoordinatesAsString, MetNowcast, Nowcast, OpenWeatherMapLocation,
    OpenWeatherNowcast,
};

use crate::AppState;

use super::{error::ApplicationError, location::lookup_location};

#[derive(Debug, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(untagged)]
pub enum LocationQuery {
    Location(City),
    Coordinates(CoordinatesAsString),
}

/// Query parameters for location-based endpoints
#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LocationParams {
    /// Location name (e.g., "Oslo")
    pub location: Option<String>,
    /// Latitude coordinate
    pub lat: Option<String>,
    /// Longitude coordinate
    pub lon: Option<String>,
}

impl LocationParams {
    pub fn into_location_query(self) -> Option<LocationQuery> {
        if let Some(location) = self.location {
            Some(LocationQuery::Location(City { location }))
        } else if let (Some(lat), Some(lon)) = (self.lat, self.lon) {
            Some(LocationQuery::Coordinates(CoordinatesAsString { lat, lon }))
        } else {
            None
        }
    }
}

pub async fn find_location(
    location_query: LocationQuery,
    client: &Client,
    location_cache: &Cache<String, OpenWeatherMapLocation>,
    apikey: &Secret<String>,
) -> anyhow::Result<Coordinates> {
    match location_query {
        LocationQuery::Location(location) => {
            let location =
                lookup_location(client, &location.location, location_cache, apikey).await;
            match location {
                Ok(location) => Ok(location.location),
                Err(err) => Err(err.into()),
            }
        }
        LocationQuery::Coordinates(cords_as_string) => {
            let cords = cords_as_string.try_into()?;
            Ok(cords)
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/met/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcast from Met.no", body = Nowcast),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 500, description = "Internal server error", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument]
pub async fn nowcast_met(
    app_state: State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Nowcast>, ApplicationError> {
    let location_query = params.into_location_query().ok_or_else(|| {
        ApplicationError::new(
            "Missing location parameter. Provide 'location' or 'lat' and 'lon'",
            StatusCode::BAD_REQUEST,
        )
    })?;

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

    match app_state
        .nowcast_cache
        .get(&format!("met_{location}"))
        .await
    {
        Some(nowcast) => return Ok(Json(nowcast)),
        None => {
            let nowcast = MetNowcast::fetch(&app_state.client, &location)
                .await
                .map_err(|err| {
                    error!("Error fetching Met.no nowcast: {:?}", err);
                    ApplicationError::new(&err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                })?;
            app_state
                .nowcast_cache
                .insert(format!("met_{location}"), nowcast.clone())
                .await;
            Ok(Json(nowcast))
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/owm/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcast from OpenWeatherMap", body = Nowcast),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 500, description = "Internal server error", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument]
pub async fn nowcast_openweathermap(
    app_state: State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Nowcast>, ApplicationError> {
    let location_query = params.into_location_query().ok_or_else(|| {
        ApplicationError::new(
            "Missing location parameter. Provide 'location' or 'lat' and 'lon'",
            StatusCode::BAD_REQUEST,
        )
    })?;

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

    match app_state
        .nowcast_cache
        .get(&format!("open_{location}"))
        .await
    {
        Some(nowcast) => return Ok(Json(nowcast)),
        None => {
            let nowcast = OpenWeatherNowcast::fetch(
                &app_state.client,
                &location,
                &app_state.openweathermap_apikey,
            )
            .await
            .map_err(|err| {
                error!("Error fetching from OpenWeatherMap.com nowcast: {:?}", err);
                ApplicationError::new(&err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
            })?;
            app_state
                .nowcast_cache
                .insert(format!("open_{location}"), nowcast.clone())
                .await;
            Ok(Json(nowcast))
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcasts from both Met.no and OpenWeatherMap", body = Vec<Nowcast>),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 500, description = "Internal server error", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument]
pub async fn nowcasts(
    app_state: State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Vec<Nowcast>>, ApplicationError> {
    let location_query = params.into_location_query().ok_or_else(|| {
        ApplicationError::new(
            "Missing location parameter. Provide 'location' or 'lat' and 'lon'",
            StatusCode::BAD_REQUEST,
        )
    })?;

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

    let open_nowcast_fut = async {
        match app_state
            .nowcast_cache
            .get(&format!("open_{location}"))
            .await
        {
            Some(nowcast) => Ok(nowcast),
            None => {
                let open_nowcast = OpenWeatherNowcast::fetch(
                    &app_state.client,
                    &location,
                    &app_state.openweathermap_apikey,
                )
                .await
                .map_err(|err| {
                    error!("Error fetching from OpenWeatherMap.com nowcast: {:?}", err);
                    ApplicationError::new(&err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                })?;
                app_state
                    .nowcast_cache
                    .insert(format!("open_{location}"), open_nowcast.clone())
                    .await;
                Ok(open_nowcast)
            }
        }
    };

    let met_nowcast_fut = async {
        match app_state
            .nowcast_cache
            .get(&format!("met_{location}"))
            .await
        {
            Some(nowcast) => Ok(nowcast),
            None => {
                let met_nowcast = MetNowcast::fetch(&app_state.client, &location)
                    .await
                    .map_err(|err| {
                        error!("Error fetching Met.no nowcast: {:?}", err);
                        ApplicationError::new(&err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                    })?;
                app_state
                    .nowcast_cache
                    .insert(format!("met_{location}"), met_nowcast.clone())
                    .await;
                Ok(met_nowcast)
            }
        }
    };

    let (open_nowcast, met_nowcast) = tokio::try_join!(open_nowcast_fut, met_nowcast_fut)?;
    Ok(Json(vec![met_nowcast, open_nowcast]))
}

#[cfg(test)]
mod tests {
    use crate::handlers::test_utils::{create_test_app, make_request};
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_nowcasts_missing_params() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/nowcasts").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_parameter_parsing() {
        let app = create_test_app();

        // Test various query parameter formats
        let test_cases = vec![
            "/api/nowcasts?location=Oslo",
            "/api/nowcasts?lat=59.91273&lon=10.74609",
            "/api/alerts?location=Bergen",
            "/api/recent_lightning?radius_km=25",
        ];

        for test_case in test_cases {
            let (status, _body) = make_request(app.clone(), test_case).await;
            // Should not return 400 Bad Request for valid query parameters
            assert_ne!(status, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn test_invalid_query_parameters() {
        let app = create_test_app();

        // Test invalid coordinate formats
        let (status, _body) = make_request(app, "/api/nowcasts?lat=invalid&lon=10.74609").await;
        assert!(status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR);
    }
}
