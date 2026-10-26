use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Url;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Parser)]
pub struct Opts {
    #[arg(short, long, required = true)]
    pub locations: Vec<String>,

    #[arg(short, long, default_value = "http://wictk.frikk.io/")]
    pub service_url: Url,

    #[arg(short = 'r', long, default_value = "http://hemrs.frikk.io/")]
    pub hemrs_url: Url,

    #[arg(long)]
    pub store_lightning: bool,

    #[arg(long, default_value = "info")]
    pub log_level: Level,
}

pub fn init_tracing(opts: &Opts) -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(opts.log_level)
        .json()
        .flatten_event(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to install tracing subscriber")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn prop_opts_parses_arbitrary_locations(locations: Vec<String>) -> bool {
            let locations: Vec<String> = locations
                .into_iter()
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .collect();
            if locations.is_empty() {
                return true;
            }

            let args = std::iter::once("client_logger".to_string()).chain(
                locations
                    .iter()
                    .flat_map(|location| ["--locations".to_string(), location.clone()]),
            );

            Opts::parse_from(args).locations == locations
        }
    }

    #[test]
    fn opts_has_expected_defaults() {
        let opts = Opts::parse_from(["client_logger", "--locations", "Trondheim"]);
        assert_eq!(opts.service_url.as_str(), "http://wictk.frikk.io/");
        assert_eq!(opts.hemrs_url.as_str(), "http://hemrs.frikk.io/");
        assert!(!opts.store_lightning);
        assert_eq!(opts.log_level, Level::INFO);
    }

    #[test]
    fn rejects_invalid_urls() {
        let result = Opts::try_parse_from([
            "client_logger",
            "--locations",
            "Trondheim",
            "--service-url",
            "not a url",
        ]);
        assert!(result.is_err());
    }
}
