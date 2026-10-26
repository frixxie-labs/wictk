mod met;
mod openweathermap;

pub use met::{MetNowcast, MET_NOWCAST_URL};
pub use openweathermap::{OpenWeatherNowcast, OPENWEATHERMAP_NOWCAST_URL};

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Nowcast {
    Met(MetNowcast),
    OpenWeather(OpenWeatherNowcast),
}

impl Display for Nowcast {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Nowcast::Met(met_nowcast) => write!(
                f,
                "{}°C, {}%, {} m/s, {}°",
                met_nowcast.air_temperature,
                met_nowcast.relative_humidity,
                met_nowcast.wind_speed,
                met_nowcast.wind_from_direction,
            ),
            Nowcast::OpenWeather(open_weather_nowcast) => write!(
                f,
                "{}°C, {}%, {} m/s, {}°",
                open_weather_nowcast.temp,
                open_weather_nowcast.humidity,
                open_weather_nowcast.wind_speed,
                open_weather_nowcast.wind_deg,
            ),
        }
    }
}
