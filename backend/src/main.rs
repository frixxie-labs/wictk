pub mod handlers;

use std::time::Duration;

use anyhow::Context;
use axum::serve;
use clap::Parser;
use handlers::{Alerts, NowcastKey};
use metrics_exporter_prometheus::PrometheusBuilder;
use moka::future::{Cache, CacheBuilder};
use redact::Secret;
use tokio::net::TcpListener;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use wictk_core::{
    Earthquake, Lightning, Nowcast, OpenWeatherMapLocation, MET_ALERTS_URL, MET_NOWCAST_URL,
    OPENWEATHERMAP_GEOCODING_URL, OPENWEATHERMAP_NOWCAST_URL, USGS_ALL_DAY_FEED_URL,
    YR_LIGHTNING_URL,
};

use crate::handlers::setup_router;

#[derive(Debug, Clone, Parser)]
pub struct Opts {
    #[arg(short, long, default_value = "0.0.0.0:3000")]
    host: String,

    #[arg(short, long, env = "OPENWEATHERMAPAPIKEY")]
    apikey: String,

    #[arg(short, long, default_value = "info")]
    log_level: Level,

    #[arg(
        long,
        env = "USGS_EARTHQUAKE_FEED_URL",
        default_value = USGS_ALL_DAY_FEED_URL
    )]
    earthquake_feed_url: String,
}

/// Upstream endpoints; overridable so tests can point them at local mock servers.
#[derive(Clone, Debug)]
pub struct ProviderUrls {
    pub met_alerts: String,
    pub met_nowcast: String,
    pub openweathermap_nowcast: String,
    pub openweathermap_geocoding: String,
    pub lightning: String,
    pub earthquakes: String,
}

impl Default for ProviderUrls {
    fn default() -> Self {
        Self {
            met_alerts: MET_ALERTS_URL.to_string(),
            met_nowcast: MET_NOWCAST_URL.to_string(),
            openweathermap_nowcast: OPENWEATHERMAP_NOWCAST_URL.to_string(),
            openweathermap_geocoding: OPENWEATHERMAP_GEOCODING_URL.to_string(),
            lightning: YR_LIGHTNING_URL.to_string(),
            earthquakes: USGS_ALL_DAY_FEED_URL.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub openweathermap_apikey: Secret<String>,
    pub client: reqwest::Client,
    pub provider_urls: ProviderUrls,
    pub alert_cache: Cache<(), Alerts>,
    /// Keyed by the normalized (trimmed, lowercased) location query.
    pub location_cache: Cache<String, OpenWeatherMapLocation>,
    pub nowcast_cache: Cache<NowcastKey, Nowcast>,
    pub lightning_cache: Cache<(), Vec<Lightning>>,
    pub earthquake_cache: Cache<(), Vec<Earthquake>>,
}

fn cache<K, V>(capacity: u64, ttl: Duration) -> Cache<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    CacheBuilder::new(capacity).time_to_live(ttl).build()
}

impl AppState {
    pub fn new(client: reqwest::Client, apikey: String, provider_urls: ProviderUrls) -> Self {
        let five_minutes = Duration::from_secs(60 * 5);
        Self {
            openweathermap_apikey: Secret::new(apikey),
            client,
            provider_urls,
            alert_cache: cache(1, five_minutes),
            location_cache: cache(20, five_minutes),
            nowcast_cache: cache(20, five_minutes),
            lightning_cache: cache(1, five_minutes),
            earthquake_cache: cache(1, Duration::from_secs(60)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(opts.log_level)
        .json()
        .flatten_event(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to install tracing subscriber")?;
    let metrics_handler = PrometheusBuilder::new()
        .install_recorder()
        .context("Failed to install Prometheus recorder")?;

    static APP_USER_AGENT: &str = concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION"),
        " ",
        env!("CARGO_PKG_HOMEPAGE"),
    );
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .build()
        .context("Failed to build HTTP client")?;

    let provider_urls = ProviderUrls {
        earthquakes: opts.earthquake_feed_url,
        ..ProviderUrls::default()
    };
    let app = setup_router(
        AppState::new(client, opts.apikey, provider_urls),
        metrics_handler,
    );

    let host = opts.host;
    let listener = TcpListener::bind(&host)
        .await
        .with_context(|| format!("Failed to bind TCP listener on {host}"))?;
    serve(listener, app)
        .await
        .with_context(|| format!("HTTP server failed while serving on {host}"))?;

    Ok(())
}
