use std::collections::HashSet;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Priority levels for notifications.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy, Serialize)]
pub enum Priority {
    Urgent,
    High,
    Default,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Urgent => "urgent",
            Priority::High => "high",
            Priority::Default => "default",
        }
    }
}

/// Tags for notifications.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy, Serialize)]
pub enum Tag {
    Wind,
    Warning,
}

impl Tag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tag::Wind => "dash",
            Tag::Warning => "warning",
        }
    }
}

/// A notification to be sent.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize)]
pub struct Notification {
    pub title: String,
    pub priority: Priority,
    pub tag: Tag,
    pub description: String,
}

impl Notification {
    pub fn new(title: String, priority: Priority, tag: Tag, description: String) -> Self {
        Self {
            title,
            priority,
            tag,
            description,
        }
    }
}

/// Response from the notification service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: String,
    pub time: u64,
    pub expires: u64,
    pub event: String,
    pub topic: String,
    pub message: String,
}

/// Notifications that have been sent and are still active.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SentLog {
    sent: HashSet<Notification>,
}

impl SentLog {
    /// The active notifications that have not been sent yet, without duplicates.
    pub fn pending(&self, active: &[Notification]) -> Vec<Notification> {
        let mut seen = HashSet::new();
        active
            .iter()
            .filter(|notification| !self.sent.contains(*notification))
            .filter(|notification| seen.insert(*notification))
            .cloned()
            .collect()
    }

    pub fn record(&mut self, notification: Notification) {
        self.sent.insert(notification);
    }

    /// Forgets notifications whose alerts have expired, so the log stays bounded
    /// and a recurring alert is notified again.
    pub fn retain_active(&mut self, active: &[Notification]) {
        let active: HashSet<&Notification> = active.iter().collect();
        self.sent
            .retain(|notification| active.contains(notification));
    }

    pub fn len(&self) -> usize {
        self.sent.len()
    }
}

/// Publishes notifications to an ntfy server.
pub struct NtfyClient {
    client: Client,
    ntfy_url: String,
}

impl NtfyClient {
    pub fn new(client: Client, ntfy_url: String) -> Self {
        Self { client, ntfy_url }
    }

    pub async fn publish(
        &self,
        notification: &Notification,
        topic: &str,
    ) -> Result<NotificationResponse> {
        let url = format!("{}/{}", self.ntfy_url.trim_end_matches('/'), topic);
        let response = self
            .client
            .post(&url)
            .header("Title", &notification.title)
            .header("Priority", notification.priority.as_str())
            .header("Tags", notification.tag.as_str())
            .body(notification.description.clone())
            .send()
            .await
            .with_context(|| format!("Failed to send notification to {url}"))?;

        if !response.status().is_success() {
            bail!("Failed to send notification: {}", response.status());
        }
        response
            .json()
            .await
            .context("Failed to decode ntfy response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, Arbitrary, Gen};

    impl Arbitrary for Notification {
        fn arbitrary(g: &mut Gen) -> Self {
            let priority = *g
                .choose(&[Priority::Urgent, Priority::High, Priority::Default])
                .unwrap();
            let tag = *g.choose(&[Tag::Wind, Tag::Warning]).unwrap();
            // A small title space makes duplicates likely.
            Notification::new(
                format!("alert-{}", u8::arbitrary(g) % 8),
                priority,
                tag,
                String::new(),
            )
        }
    }

    fn sent(log: &SentLog, notifications: &[Notification]) -> SentLog {
        notifications
            .iter()
            .cloned()
            .fold(log.clone(), |mut log, n| {
                log.record(n);
                log
            })
    }

    quickcheck! {
        fn prop_nothing_is_pending_after_sending_it(active: Vec<Notification>) -> bool {
            let log = SentLog::default();
            let log = sent(&log, &log.pending(&active));
            log.pending(&active).is_empty()
        }

        fn prop_pending_has_no_duplicates(active: Vec<Notification>) -> bool {
            let pending = SentLog::default().pending(&active);
            pending.iter().collect::<HashSet<_>>().len() == pending.len()
        }

        fn prop_retain_active_bounds_log_to_active(old: Vec<Notification>, active: Vec<Notification>) -> bool {
            let mut log = sent(&SentLog::default(), &old);
            log.retain_active(&active);
            log.len() <= active.len() && log.sent.iter().all(|n| active.contains(n))
        }
    }

    #[tokio::test]
    async fn publishes_notification() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/test_topic")
            .match_header("Title", "Test Alert")
            .match_header("Priority", "high")
            .match_header("Tags", "warning")
            .match_body("This is a test alert")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "id": "test-id",
                    "time": 123,
                    "expires": 456,
                    "event": "message",
                    "topic": "test_topic",
                    "message": "This is a test alert"
                }"#,
            )
            .create_async()
            .await;

        let notifier = NtfyClient::new(Client::new(), server.url());
        let notification = Notification::new(
            "Test Alert".to_string(),
            Priority::High,
            Tag::Warning,
            "This is a test alert".to_string(),
        );

        let response = notifier.publish(&notification, "test_topic").await;

        assert!(response.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn reports_unsuccessful_publish() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/test_topic")
            .with_status(500)
            .create_async()
            .await;
        let notifier = NtfyClient::new(Client::new(), server.url());
        let notification = Notification::new(
            "Test Alert".to_string(),
            Priority::High,
            Tag::Warning,
            String::new(),
        );

        assert!(notifier.publish(&notification, "test_topic").await.is_err());
    }
}
