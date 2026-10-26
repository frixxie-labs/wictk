mod met;

pub use met::{AlertEvent, Area, Certainty, MetAlert, TimeDuration, MET_ALERTS_URL};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub enum Alert {
    /// The alert was issued by the Norwegian Meteorological Institute.
    Met(MetAlert),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Severity {
    /// The alert is for a moderate event.
    Yellow,
    /// The alert is for a severe event.
    Orange,
    /// The alert is for an extreme event.
    Red,
}
