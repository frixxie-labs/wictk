use axum::{
    extract::{Query, State},
    Json,
};
use tracing::instrument;
use wictk_core::{Coordinates, MetNowcast, Nowcast, OpenWeatherNowcast};

use crate::AppState;

use super::{
    cache::get_or_fetch,
    error::ApplicationError,
    location::{resolve_location, LocationParams},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NowcastSource {
    Met,
    OpenWeatherMap,
}

pub type NowcastKey = (NowcastSource, Coordinates);

async fn nowcast(
    app_state: &AppState,
    source: NowcastSource,
    location: Coordinates,
) -> Result<Nowcast, ApplicationError> {
    let client = &app_state.client;
    let urls = &app_state.provider_urls;
    let cache = &app_state.nowcast_cache;
    match source {
        NowcastSource::Met => {
            get_or_fetch(
                cache,
                (source, location),
                MetNowcast::fetch(client, &urls.met_nowcast, &location),
                "Met.no nowcast",
            )
            .await
        }
        NowcastSource::OpenWeatherMap => {
            get_or_fetch(
                cache,
                (source, location),
                OpenWeatherNowcast::fetch(
                    client,
                    &urls.openweathermap_nowcast,
                    &location,
                    &app_state.openweathermap_apikey,
                ),
                "OpenWeatherMap nowcast",
            )
            .await
        }
    }
}

async fn required_location(
    app_state: &AppState,
    params: LocationParams,
) -> Result<Coordinates, ApplicationError> {
    resolve_location(app_state, params.parse_required()?).await
}

#[utoipa::path(
    get,
    path = "/api/met/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcast from Met.no", body = Nowcast),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "Upstream provider failure", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument(skip(app_state))]
pub async fn nowcast_met(
    State(app_state): State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Nowcast>, ApplicationError> {
    let location = required_location(&app_state, params).await?;
    nowcast(&app_state, NowcastSource::Met, location)
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/api/owm/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcast from OpenWeatherMap", body = Nowcast),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "Upstream provider failure", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument(skip(app_state))]
pub async fn nowcast_openweathermap(
    State(app_state): State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Nowcast>, ApplicationError> {
    let location = required_location(&app_state, params).await?;
    nowcast(&app_state, NowcastSource::OpenWeatherMap, location)
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/api/nowcasts",
    params(LocationParams),
    responses(
        (status = 200, description = "Weather nowcasts from both Met.no and OpenWeatherMap", body = Vec<Nowcast>),
        (status = 400, description = "Bad request - missing or invalid parameters"),
        (status = 404, description = "Location not found", body = String),
        (status = 502, description = "Upstream provider failure", body = String)
    ),
    tag = "nowcasts"
)]
#[instrument(skip(app_state))]
pub async fn nowcasts(
    State(app_state): State<AppState>,
    Query(params): Query<LocationParams>,
) -> Result<Json<Vec<Nowcast>>, ApplicationError> {
    let location = required_location(&app_state, params).await?;
    let (met, open_weather) = tokio::try_join!(
        nowcast(&app_state, NowcastSource::Met, location),
        nowcast(&app_state, NowcastSource::OpenWeatherMap, location),
    )?;
    Ok(Json(vec![met, open_weather]))
}

#[cfg(test)]
mod tests {
    use crate::handlers::test_utils::{
        create_test_app, create_test_app_with_state, make_request, test_state,
    };
    use axum::http::StatusCode;
    use wictk_core::Nowcast;

    const MET_FIXTURE: &str = include_str!("../../../wictk_core/tests/fixtures/met_nowcast.json");
    const OWM_FIXTURE: &str =
        include_str!("../../../wictk_core/tests/fixtures/openweathermap_nowcast.json");

    #[tokio::test]
    async fn test_nowcasts_missing_params() {
        let app = create_test_app();
        let (status, _body) = make_request(app, "/api/nowcasts").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rejects_invalid_coordinates() {
        for query in [
            "/api/nowcasts?lat=invalid&lon=10.74609",
            "/api/nowcasts?lat=95&lon=10.74609",
            "/api/nowcasts?lat=59.9",
        ] {
            let (status, _body) = make_request(create_test_app(), query).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        }
    }

    #[tokio::test]
    async fn fetches_both_nowcasts_once_and_caches_them() {
        let mut server = mockito::Server::new_async().await;
        let met = server
            .mock("GET", "/met")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(MET_FIXTURE)
            .expect(1)
            .create_async()
            .await;
        let owm = server
            .mock("GET", "/owm")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(OWM_FIXTURE)
            .expect(1)
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.met_nowcast = format!("{}/met", server.url());
        state.provider_urls.openweathermap_nowcast = format!("{}/owm", server.url());
        let app = create_test_app_with_state(state);

        let (status, body) = make_request(app.clone(), "/api/nowcasts?lat=63.43&lon=10.40").await;
        let (cached_status, _) = make_request(app, "/api/met/nowcasts?lat=63.43&lon=10.40").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(cached_status, StatusCode::OK);
        let nowcasts: Vec<Nowcast> = serde_json::from_slice(&body).unwrap();
        assert!(matches!(
            nowcasts.as_slice(),
            [Nowcast::Met(_), Nowcast::OpenWeather(_)]
        ));
        met.assert_async().await;
        owm.assert_async().await;
    }

    #[tokio::test]
    async fn maps_provider_failure_to_bad_gateway() {
        let mut server = mockito::Server::new_async().await;
        let _met = server
            .mock("GET", "/met")
            .match_query(mockito::Matcher::Any)
            .with_status(500)
            .create_async()
            .await;
        let mut state = test_state();
        state.provider_urls.met_nowcast = format!("{}/met", server.url());

        let (status, _) = make_request(
            create_test_app_with_state(state),
            "/api/met/nowcasts?lat=63.43&lon=10.40",
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }
}
