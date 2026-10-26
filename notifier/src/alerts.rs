use wictk_core::{AlertEvent, MetAlert, Severity};

use crate::notifications::{Notification, Priority, Tag};

impl From<Severity> for Priority {
    fn from(value: Severity) -> Self {
        match value {
            Severity::Yellow => Priority::Default,
            Severity::Orange => Priority::High,
            Severity::Red => Priority::Urgent,
        }
    }
}

impl From<&AlertEvent> for Tag {
    fn from(event: &AlertEvent) -> Self {
        match event {
            AlertEvent::Gale => Tag::Wind,
            _ => Tag::Warning,
        }
    }
}

impl From<MetAlert> for Notification {
    fn from(alert: MetAlert) -> Self {
        Notification::new(
            alert.title,
            alert.severity.into(),
            Tag::from(&alert.event),
            alert.description,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wictk_core::{Area, Certainty, TimeDuration};

    fn alert(severity: Severity, event: AlertEvent) -> MetAlert {
        MetAlert {
            title: "Kuling".to_string(),
            severity,
            description: "Sterk kuling".to_string(),
            certainty: Certainty::Likely,
            event,
            duration: TimeDuration {
                from: Utc::now(),
                until: Utc::now(),
            },
            area: Area::Single(vec![]),
        }
    }

    #[test]
    fn maps_severity_to_priority() {
        let cases = [
            (Severity::Yellow, Priority::Default),
            (Severity::Orange, Priority::High),
            (Severity::Red, Priority::Urgent),
        ];

        for (severity, priority) in cases {
            assert_eq!(Priority::from(severity), priority);
        }
    }

    #[test]
    fn maps_event_to_tag() {
        let cases = [
            (AlertEvent::Gale, Tag::Wind),
            (AlertEvent::ForestFire, Tag::Warning),
            (AlertEvent::Other("unknown".to_string()), Tag::Warning),
        ];

        for (event, tag) in cases {
            assert_eq!(Tag::from(&event), tag);
        }
    }

    #[test]
    fn builds_notification_from_alert() {
        let notification = Notification::from(alert(Severity::Orange, AlertEvent::Gale));

        assert_eq!(
            notification,
            Notification::new(
                "Kuling".to_string(),
                Priority::High,
                Tag::Wind,
                "Sterk kuling".to_string()
            )
        );
    }
}
