use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Serialize, Deserialize, PartialEq, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct City {
    /// Location name to search for (e.g., "Oslo", "London")
    pub location: String,
}
