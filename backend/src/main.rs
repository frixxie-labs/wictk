pub mod handlers;

use anyhow::Context;
use axum::serve;
use clap::Parser;
use handlers::Alerts;
use metrics_exporter_prometheus::PrometheusBuilder;
use moka::future::{Cache, CacheBuilder};
use redact::Secret;
use tokio::net::TcpListener;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use wictk_core::{Lightning, Nowcast, OpenWeatherMapLocation};

use crate::handlers::setup_router;

#[derive(Debug, Clone)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::str::FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err("unknown log level".to_string()),
        }
    }
}

#[derive(Debug, Clone, Parser)]
pub struct Opts {
    #[arg(short, long, default_value = "0.0.0.0:3000")]
    host: String,

    #[arg(short, long, env = "OPENWEATHERMAPAPIKEY")]
    apikey: String,

    #[arg(short, long, default_value = "info")]
    log_level: LogLevel,
}

impl From<LogLevel> for Level {
    fn from(log_level: LogLevel) -> Self {
        match log_level {
            LogLevel::Trace => Level::TRACE,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub openweathermap_apikey: Secret<String>,
    pub client: reqwest::Client,
    pub alert_cache: Cache<String, Alerts>,
    pub location_cache: Cache<String, OpenWeatherMapLocation>,
    pub nowcast_cache: Cache<String, Nowcast>,
    pub lightning_cache: Cache<String, Vec<Lightning>>,
}

impl AppState {
    pub fn new(client: reqwest::Client, apikey: String) -> Self {
        Self {
            openweathermap_apikey: Secret::new(apikey),
            client,
            alert_cache: CacheBuilder::new(1)
                .time_to_live(std::time::Duration::from_secs(60 * 5))
                .build(),
            location_cache: CacheBuilder::new(20)
                .time_to_live(std::time::Duration::from_secs(60 * 5))
                .build(),
            nowcast_cache: CacheBuilder::new(20)
                .time_to_live(std::time::Duration::from_secs(60 * 5))
                .build(),
            lightning_cache: CacheBuilder::new(1)
                .time_to_live(std::time::Duration::from_secs(60 * 5))
                .build(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::parse();

    let level: Level = opts.log_level.into();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .json()
        .flatten_event(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();
    let metrics_handler = PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install recorder/exporter");

    let client_builder = reqwest::Client::builder();
    static APP_USER_AGENT: &str = concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION"),
        " ",
        env!("CARGO_PKG_HOMEPAGE"),
    );
    let client = client_builder.user_agent(APP_USER_AGENT).build().unwrap();

    let app_state = AppState::new(client, opts.apikey);

    let app = setup_router(app_state, metrics_handler);

    let host = opts.host;
    let listener = TcpListener::bind(&host)
        .await
        .with_context(|| format!("Failed to bind TCP listener on {host}"))?;
    serve(listener, app)
        .await
        .with_context(|| format!("HTTP server failed while serving on {host}"))?;

    Ok(())
}
