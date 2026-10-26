use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use wictk_core::Alert;

use crate::notifications::{Notification, NtfyClient, SentLog};

mod alerts;
mod notifications;

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
    log_level: Level,

    /// Sleep duration between checks (in seconds)
    #[arg(short, long, default_value = "120", env = "SLEEP_DURATION")]
    sleep: u64,

    /// Location (city name)
    #[arg(short, long, default_value = "Trondheim", env = "LOCATION")]
    location: String,
}

pub async fn get_alerts(client: &Client, url: &str, location: &str) -> Result<Vec<Alert>> {
    client
        .get(url)
        .query(&[("location", location)])
        .send()
        .await
        .with_context(|| format!("Failed to fetch alerts for '{location}' from {url}"))?
        .error_for_status()
        .with_context(|| format!("Alerts endpoint {url} returned an error"))?
        .json()
        .await
        .with_context(|| format!("Failed to parse alerts for '{location}' from {url}"))
}

fn to_notifications(alerts: Vec<Alert>) -> Vec<Notification> {
    alerts
        .into_iter()
        .map(|Alert::Met(alert)| alert.into())
        .collect()
}

async fn notify_new_alerts(
    client: &Client,
    ntfy: &NtfyClient,
    sent: &mut SentLog,
    opts: &Opts,
) -> Result<()> {
    let active = to_notifications(get_alerts(client, &opts.alerts_url, &opts.location).await?);
    tracing::info!("Fetched {} alerts", active.len());

    sent.retain_active(&active);
    for notification in sent.pending(&active) {
        match ntfy.publish(&notification, &opts.topic).await {
            Ok(_) => {
                tracing::info!("Notification sent: {}", notification.title);
                sent.record(notification);
            }
            Err(err) => tracing::warn!(error = ?err, "Notification was not sent"),
        }
    }
    tracing::info!("{} active notifications sent", sent.len());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse();
    let subscriber = FmtSubscriber::builder()
        .with_max_level(opts.log_level)
        .json()
        .flatten_event(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to install tracing subscriber")?;

    let client = Client::new();
    let ntfy = NtfyClient::new(client.clone(), opts.ntfy_url.clone());
    let mut sent = SentLog::default();

    tracing::info!("Starting notifier with configuration: {:?}", opts);
    loop {
        if let Err(err) = notify_new_alerts(&client, &ntfy, &mut sent, &opts).await {
            tracing::error!(error = ?err, "Failed to process alerts; retrying next cycle");
        }
        tokio::time::sleep(Duration::from_secs(opts.sleep)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetches_alerts_with_encoded_location() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/alerts")
            .match_query(mockito::Matcher::UrlEncoded(
                "location".into(),
                "Bodø & Salten".into(),
            ))
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let alerts = get_alerts(
            &Client::new(),
            &format!("{}/api/alerts", server.url()),
            "Bodø & Salten",
        )
        .await
        .unwrap();

        assert!(alerts.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn reports_unsuccessful_alert_fetch() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/alerts")
            .match_query(mockito::Matcher::Any)
            .with_status(502)
            .create_async()
            .await;

        let result = get_alerts(
            &Client::new(),
            &format!("{}/api/alerts", server.url()),
            "Trondheim",
        )
        .await;

        assert!(result.is_err());
    }
}
