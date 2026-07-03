use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;
use std::time::Duration;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use wictk_core::Alert;

use crate::notifications::{Notifier, NtfyNotifier};
mod alerts;
mod notifications;

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

#[derive(Debug, Parser, Clone)]
#[command(name = "notifier", about = "Notification CLI for WICTK")]
pub struct Opts {
    /// Ntfy server URL
    #[arg(short, long, default_value = "https://ntfy.frikk.io", env = "NTFY_URL")]
    ntfy_url: String,

    /// WICTK API alerts endpoint
    #[arg(
        short,
        long,
        default_value = "http://wictk/api/alerts",
        env = "WICTK_ALERTS_URL"
    )]
    alerts_url: String,

    /// Notification topic
    #[arg(short, long, default_value = "weather_alerts", env = "NTFY_TOPIC")]
    topic: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: LogLevel,

    /// Sleep duration between checks (in seconds)
    #[arg(short, long, default_value = "120", env = "SLEEP_DURATION")]
    sleep: u64,

    /// Location (city name or coordinates)
    #[arg(short, long, default_value = "Trondheim", env = "LOCATION")]
    location: String,
}

pub async fn get_met_alerts(client: &Client, url: &str, location: &str) -> Result<Vec<Alert>> {
    let resp = client
        .get(format!("{url}?location={}", location))
        .send()
        .await
        .with_context(|| format!("Failed to fetch alerts for location '{location}' from {url}"))?;
    let alerts: Vec<Alert> = resp.json().await.with_context(|| {
        format!("Failed to parse alerts response for location '{location}' from {url}")
    })?;
    Ok(alerts)
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse();
    let level: Level = opts.log_level.clone().into();
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    let client = reqwest::Client::new();
    let mut alerter = NtfyNotifier::new(client.clone(), opts.ntfy_url.clone());
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("Starting notifier with configuration: {:?}", opts);
    loop {
        let alerts = get_met_alerts(&client, &opts.alerts_url, &opts.location).await?;
        tracing::info!("Fetched {} alerts", alerts.len());
        for alert in alerts {
            match alert {
                Alert::Met(alert) => {
                    let notification: crate::notifications::Notification = alert.into();
                    match alerter.publish(notification, &opts.topic).await {
                        Ok(_) => {
                            tracing::info!("Notification sent");
                        }
                        Err(e) => {
                            tracing::warn!("Notification was not sent: {}", e);
                        }
                    }
                }
                _ => tracing::warn!("Unknown alert type"),
            }
        }
        let sent_alerts = alerter.notifications(&opts.topic).await;
        match sent_alerts {
            Ok(alerts) => {
                tracing::info!("Sent {} notifications", alerts.len());
            }
            Err(e) => {
                tracing::warn!("Could not fetch sent notifications: {}", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(opts.sleep)).await;
    }
}
