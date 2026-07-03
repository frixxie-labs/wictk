use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::str::FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err("unknown log level".to_string()),
        }
    }
}

impl From<LogLevel> for Level {
    fn from(log_level: LogLevel) -> Self {
        match log_level {
            LogLevel::Trace => Level::TRACE,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }
}

#[derive(Debug, Parser)]
pub struct Opts {
    #[arg(short, long, required = true)]
    pub locations: Vec<String>,

    #[arg(short, long, default_value = "http://wictk.frikk.io/")]
    pub service_url: String,

    #[arg(short = 'r', long, default_value = "http://hemrs.frikk.io/")]
    pub hemrs_url: String,

    #[arg(long)]
    pub store_lightning: bool,

    #[arg(long, default_value = "info")]
    pub log_level: LogLevel,
}

pub fn init_tracing(opts: &Opts) {
    let level: Level = opts.log_level.clone().into();
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, Arbitrary, Gen};

    const VALID_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

    impl Arbitrary for LogLevel {
        fn arbitrary(g: &mut Gen) -> Self {
            let variants = [
                LogLevel::Trace,
                LogLevel::Debug,
                LogLevel::Info,
                LogLevel::Warn,
                LogLevel::Error,
            ];
            variants[usize::arbitrary(g) % variants.len()].clone()
        }
    }

    fn log_level_to_str(level: &LogLevel) -> &'static str {
        match level {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    fn expected_tracing_level(level: &LogLevel) -> Level {
        match level {
            LogLevel::Trace => Level::TRACE,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }

    quickcheck! {
        fn prop_valid_log_level_round_trips_through_from_str(level: LogLevel) -> bool {
            let s = log_level_to_str(&level);
            let parsed: LogLevel = s.parse().unwrap();
            log_level_to_str(&parsed) == s
        }

        fn prop_log_level_converts_to_correct_tracing_level(level: LogLevel) -> bool {
            let expected = expected_tracing_level(&level);
            let actual: Level = level.into();
            actual == expected
        }

        fn prop_invalid_strings_fail_to_parse(s: String) -> bool {
            if VALID_LEVELS.contains(&s.as_str()) {
                // Valid strings should parse successfully
                s.parse::<LogLevel>().is_ok()
            } else {
                // Everything else must fail
                s.parse::<LogLevel>().is_err()
            }
        }

        fn prop_opts_parses_arbitrary_locations(locations: Vec<String>) -> bool {
            // Filter to non-empty strings (empty strings would be ambiguous CLI args)
            let locations: Vec<String> = locations
                .into_iter()
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .collect();

            if locations.is_empty() {
                return true; // vacuously true, --locations is required
            }

            let mut args: Vec<String> = vec!["client_logger".to_string()];
            for loc in &locations {
                args.push("--locations".to_string());
                args.push(loc.clone());
            }

            let opts = Opts::parse_from(&args);
            opts.locations == locations
        }

        fn prop_opts_preserves_custom_urls(service_url: String, hemrs_url: String) -> bool {
            // Skip strings that look like flags or are empty
            if service_url.is_empty()
                || hemrs_url.is_empty()
                || service_url.starts_with('-')
                || hemrs_url.starts_with('-')
            {
                return true;
            }

            let args = vec![
                "client_logger",
                "--locations", "TestCity",
                "--service-url", &service_url,
                "--hemrs-url", &hemrs_url,
            ];

            let opts = Opts::parse_from(&args);
            opts.service_url == service_url && opts.hemrs_url == hemrs_url
        }
    }

    #[test]
    fn opts_has_expected_defaults() {
        let opts = Opts::parse_from(["client_logger", "--locations", "Trondheim"]);
        assert_eq!(opts.service_url, "http://wictk.frikk.io/");
        assert_eq!(opts.hemrs_url, "http://hemrs.frikk.io/");
        assert!(!opts.store_lightning);
        assert!(matches!(opts.log_level, LogLevel::Info));
    }
}
