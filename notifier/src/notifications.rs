use std::collections::{HashMap, HashSet};
use std::fmt;

use anyhow::Result;
use reqwest::Client;
use serde::Serialize;

/// Priority levels for notifications.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize)]
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
#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize)]
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

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
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
    /// Create a new notification.
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationResponse {
    pub id: String,
    pub time: u64,
    pub expires: u64,
    pub event: String,
    pub topic: String,
    pub message: String,
}

/// Trait for sending and querying notifications.
pub trait Notifier {
    async fn publish(&mut self, alert: Notification, topic: &str) -> Result<NotificationResponse>;

    async fn notifications(&self, topic: &str) -> Result<Vec<Notification>>;

    async fn has_sent_alert(&self, alert: &Notification, topic: &str) -> Result<bool> {
        let notifications = self.notifications(topic).await?;
        Ok(notifications.contains(alert))
    }
}

impl Notifier for NtfyNotifier {
    async fn publish(&mut self, alert: Notification, topic: &str) -> Result<NotificationResponse> {
        if self.has_sent_alert(&alert, topic).await? {
            anyhow::bail!("Notification already sent");
        }
        let alert_clone = alert.clone();
        let url = format!("{}/{}", self.ntfy_url, topic);
        let resp = self
            .client
            .post(&url)
            .header("Title", &alert.title)
            .header("Priority", alert.priority.as_str())
            .header("Tags", alert.tag.as_str())
            .body(alert.description.clone())
            .send()
            .await?;

        if resp.status().is_success() {
            let alert_resp: NotificationResponse = resp.json().await?;
            self.add_alert(alert_clone, topic);
            Ok(alert_resp)
        } else {
            anyhow::bail!("Failed to send notification: {}", resp.status());
        }
    }

    async fn notifications(&self, topic: &str) -> Result<Vec<Notification>> {
        Ok(self
            .sent_notifications
            .get(topic)
            .map(|n| n.iter().cloned().collect())
            .unwrap_or_default())
    }
}

/// Implementation of Notifier using ntfy.sh
pub struct NtfyNotifier {
    sent_notifications: HashMap<String, HashSet<Notification>>,
    client: Client,
    ntfy_url: String,
}

impl NtfyNotifier {
    /// Create a new NtfyNotifier.
    pub fn new(client: Client, ntfy_url: String) -> Self {
        Self {
            sent_notifications: HashMap::new(),
            client,
            ntfy_url,
        }
    }

    /// Add an alert to the sent notifications for a topic.
    pub fn add_alert(&mut self, alert: Notification, topic: &str) {
        self.sent_notifications
            .entry(topic.to_string())
            .or_default()
            .insert(alert);
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[tokio::test]
    async fn should_publish_notification_ok() {
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

        let client = reqwest::Client::new();
        let mut notifier = NtfyNotifier::new(client, server.url());
        let alert = Notification::new(
            "Test Alert".to_string(),
            Priority::High,
            Tag::Warning,
            "This is a test alert".to_string(),
        );
        let topic = "test_topic".to_string();
        let response = notifier.publish(alert, &topic).await;
        assert!(response.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn should_get_no_notifications() {
        let client = reqwest::Client::new();
        let notifier = NtfyNotifier::new(client, "https://ntfy.frikk.io".to_string());
        let topic = "non_existent_topic".to_string();
        let notifications = notifier.notifications(&topic).await;
        assert!(notifications.unwrap().is_empty());
    }

    #[tokio::test]
    async fn should_add_and_get_notifications() {
        let client = reqwest::Client::new();
        let mut notifier = NtfyNotifier::new(client, "https://ntfy.frikk.io".to_string());
        let alert = Notification::new(
            "Test Alert".to_string(),
            Priority::High,
            Tag::Warning,
            "This is a test alert".to_string(),
        );
        let topic = "test_topic".to_string();
        notifier.add_alert(alert.clone(), &topic);
        let notifications = notifier.notifications(&topic).await.unwrap();
        assert!(notifications.contains(&alert));
    }

    #[tokio::test]
    async fn should_check_sent_alert() {
        let client = reqwest::Client::new();
        let mut notifier = NtfyNotifier::new(client, "https://ntfy.frikk.io".to_string());
        let alert = Notification::new(
            "Test Alert".to_string(),
            Priority::High,
            Tag::Warning,
            "This is a test alert".to_string(),
        );
        let topic = "test_topic".to_string();
        notifier.add_alert(alert.clone(), &topic);
        let has_sent = notifier.has_sent_alert(&alert, &topic).await.unwrap();
        assert!(has_sent);
    }
}
