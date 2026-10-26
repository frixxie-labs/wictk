use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// OpenAPI schema for `geo::Point`: longitude (x) and latitude (y).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GeoPoint {
    /// Longitude
    pub x: f64,
    /// Latitude
    pub y: f64,
}
