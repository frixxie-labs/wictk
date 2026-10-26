use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use geo::Point;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;
use utoipa::ToSchema;

use crate::geo_point::GeoPoint;

use super::{Alert, Severity};

pub const MET_ALERTS_URL: &str = "https://api.met.no/weatherapi/metalerts/2.0/current.json";

impl From<MetAlert> for Alert {
    fn from(met: MetAlert) -> Self {
        Alert::Met(met)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub enum Area {
    #[schema(value_type = Vec<GeoPoint>)]
    Single(Vec<Point>),
    #[schema(value_type = Vec<Vec<GeoPoint>>)]
    Multiple(Vec<Vec<Point>>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TimeDuration {
    pub from: DateTime<Utc>,
    pub until: DateTime<Utc>,
}

/// CAP certainty of an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Certainty {
    Observed,
    Likely,
    Possible,
    Unlikely,
    #[serde(other)]
    Unknown,
}

/// The weather phenomenon an alert is about. Unrecognised events keep their original name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AlertEvent {
    Avalanches,
    BlowingSnow,
    DrivingConditions,
    Flood,
    ForestFire,
    Gale,
    Ice,
    IcingOnShips,
    Lightning,
    PolarLow,
    Rain,
    RainFlood,
    Snow,
    StormSurge,
    Wind,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MetAlert {
    pub title: String,
    pub severity: Severity,
    pub description: String,
    pub certainty: Certainty,
    pub event: AlertEvent,
    pub duration: TimeDuration,
    pub area: Area,
}

#[derive(Deserialize)]
struct MetAlertsResponse {
    features: Vec<Value>,
}

#[derive(Deserialize)]
struct MetFeature {
    geometry: MetGeometry,
    properties: MetProperties,
    when: MetWhen,
}

/// GeoJSON positions are `[lon, lat, ...]`; polygons are lists of rings.
type Ring = Vec<Vec<f64>>;

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum MetGeometry {
    Polygon(Vec<Ring>),
    MultiPolygon(Vec<Vec<Ring>>),
}

#[derive(Deserialize)]
struct MetProperties {
    title: String,
    description: String,
    severity: MetSeverity,
    certainty: Certainty,
    event: AlertEvent,
}

#[derive(Deserialize)]
enum MetSeverity {
    Moderate,
    Severe,
    Extreme,
}

impl From<MetSeverity> for Severity {
    fn from(severity: MetSeverity) -> Self {
        match severity {
            MetSeverity::Moderate => Severity::Yellow,
            MetSeverity::Severe => Severity::Orange,
            MetSeverity::Extreme => Severity::Red,
        }
    }
}

#[derive(Deserialize)]
struct MetWhen {
    interval: (DateTime<Utc>, DateTime<Utc>),
}

fn to_point(position: &[f64]) -> Result<Point> {
    match position {
        [lon, lat, ..] => Ok(Point::new(*lon, *lat)),
        _ => bail!("Expected a [lon, lat] position, got {position:?}"),
    }
}

/// Only the exterior ring is kept, since `Area` cannot represent holes.
fn exterior_ring(polygon: &[Ring]) -> Result<Vec<Point>> {
    polygon
        .first()
        .context("Polygon has no rings")?
        .iter()
        .map(|position| to_point(position))
        .collect()
}

impl TryFrom<MetGeometry> for Area {
    type Error = anyhow::Error;

    fn try_from(geometry: MetGeometry) -> Result<Self> {
        match geometry {
            MetGeometry::Polygon(polygon) => exterior_ring(&polygon).map(Area::Single),
            MetGeometry::MultiPolygon(polygons) => polygons
                .iter()
                .map(|polygon| exterior_ring(polygon))
                .collect::<Result<_>>()
                .map(Area::Multiple),
        }
    }
}

impl TryFrom<MetFeature> for MetAlert {
    type Error = anyhow::Error;

    fn try_from(feature: MetFeature) -> Result<Self> {
        let (from, until) = feature.when.interval;
        Ok(MetAlert {
            title: feature.properties.title,
            severity: feature.properties.severity.into(),
            description: feature.properties.description,
            certainty: feature.properties.certainty,
            event: feature.properties.event,
            duration: TimeDuration { from, until },
            area: feature.geometry.try_into()?,
        })
    }
}

fn parse_feature(feature: Value) -> Result<MetAlert> {
    MetAlert::try_from(serde_json::from_value::<MetFeature>(feature)?)
}

/// Malformed features are logged and skipped so one bad alert does not hide the rest.
fn parse_alerts(response: MetAlertsResponse) -> Vec<Alert> {
    response
        .features
        .into_iter()
        .filter_map(|feature| {
            parse_feature(feature)
                .inspect_err(|err| warn!(error = ?err, "Skipping malformed Met.no alert"))
                .ok()
        })
        .map(Alert::from)
        .collect()
}

impl MetAlert {
    pub async fn fetch(client: &Client, url: &str) -> Result<Vec<Alert>> {
        let response = client
            .get(url)
            .send()
            .await
            .context("Request to Met.no failed")?
            .error_for_status()
            .context("Met.no alerts returned an error")?
            .json::<MetAlertsResponse>()
            .await
            .context("Deserialization from Met.no failed")?;

        Ok(parse_alerts(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    const MULTIPOLYGON_FIXTURE: &str =
        include_str!("../../tests/fixtures/met_alerts_multipolygon.json");
    const POLYGON_FIXTURE: &str = include_str!("../../tests/fixtures/met_alert_polygon.json");

    #[test]
    fn parses_multi_polygon_alerts() -> Result<()> {
        let alerts = parse_alerts(serde_json::from_str(MULTIPOLYGON_FIXTURE)?);

        assert_eq!(alerts.len(), 3);
        let Alert::Met(alert) = &alerts[0];
        match &alert.area {
            Area::Multiple(polygons) => {
                assert_eq!(polygons.len(), 2);
                assert_eq!(polygons[0].len(), 11);
                assert_eq!(polygons[1].len(), 9);
            }
            Area::Single(_) => panic!("expected a multi polygon"),
        }
        assert_eq!(alert.event, AlertEvent::Gale);
        assert_eq!(alert.certainty, Certainty::Observed);
        Ok(())
    }

    #[test]
    fn parses_polygon_alert() -> Result<()> {
        let alert = parse_feature(serde_json::from_str(POLYGON_FIXTURE)?)?;

        assert_eq!(alert.severity, Severity::Yellow);
        assert_eq!(
            alert.title,
            "Skogbrannfare, gult nivå, Lofoten, Vesterålen, og deler av Salten, Ofoten og Sør-Troms, 2023-08-10T22:00:00+00:00, 2023-08-14T22:00:00+00:00"
        );
        assert_eq!(
            alert.description,
            "Update: Lokal skog- og lyngbrannfare inntil det kommer nedbør av betydning."
        );
        assert_eq!(alert.certainty, Certainty::Likely);
        assert_eq!(alert.event, AlertEvent::ForestFire);
        assert_eq!(
            alert.duration.from.to_rfc3339(),
            "2023-08-10T22:00:00+00:00"
        );
        assert_eq!(
            alert.duration.until.to_rfc3339(),
            "2023-08-14T22:00:00+00:00"
        );
        assert!(matches!(alert.area, Area::Single(ref points) if points.len() == 21));
        Ok(())
    }

    #[test]
    fn rejects_malformed_positions_instead_of_inventing_points() -> Result<()> {
        let mut feature: Value = serde_json::from_str(POLYGON_FIXTURE)?;
        feature["geometry"]["coordinates"][0][0] = json!([15.4]);

        assert!(parse_feature(feature).is_err());
        Ok(())
    }

    #[test]
    fn skips_malformed_features_but_keeps_valid_ones() -> Result<()> {
        let valid: Value = serde_json::from_str(POLYGON_FIXTURE)?;
        let response = MetAlertsResponse {
            features: vec![json!({ "garbage": true }), valid],
        };

        assert_eq!(parse_alerts(response).len(), 1);
        Ok(())
    }

    #[test]
    fn maps_severity_levels() {
        assert_eq!(Severity::from(MetSeverity::Moderate), Severity::Yellow);
        assert_eq!(Severity::from(MetSeverity::Severe), Severity::Orange);
        assert_eq!(Severity::from(MetSeverity::Extreme), Severity::Red);
    }

    #[test]
    fn unknown_events_round_trip_with_their_name() -> Result<()> {
        let event: AlertEvent = serde_json::from_value(json!("someNewEvent"))?;

        assert_eq!(event, AlertEvent::Other("someNewEvent".to_string()));
        assert_eq!(serde_json::to_value(&event)?, json!("someNewEvent"));
        assert_eq!(
            serde_json::to_value(AlertEvent::ForestFire)?,
            json!("forestFire")
        );
        Ok(())
    }

    #[tokio::test]
    async fn fetches_alerts() -> Result<()> {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/alerts.json")
            .with_status(200)
            .with_body(MULTIPOLYGON_FIXTURE)
            .create_async()
            .await;

        let alerts =
            MetAlert::fetch(&Client::new(), &format!("{}/alerts.json", server.url())).await?;

        mock.assert_async().await;
        assert_eq!(alerts.len(), 3);
        Ok(())
    }
}
