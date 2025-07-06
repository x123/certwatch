//! Command-Line Interface (CLI) argument parsing.
//!
//! This module defines the command-line arguments for the application using the
//! `clap` crate. These arguments are parsed at startup and then merged with
//! the configuration from the `certwatch.toml` file and environment variables.

use clap::Parser;
use figment::{
    value::{Dict, Map, Value},
    Error, Metadata, Profile, Provider,
};
use std::path::PathBuf;

/// A high-performance, real-time Certificate Transparency Log monitor.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Deduplication window duration in seconds.
    #[arg(long, value_name = "SECONDS")]
    pub dedup_window: Option<u64>,

    /// Sampling rate for the certstream (0.0 to 1.0).
    #[arg(long, value_name = "RATE")]
    pub sample_rate: Option<f64>,

    /// Timeout for DNS resolution in milliseconds.
    #[arg(long, value_name = "MS")]
    pub dns_timeout_ms: Option<u64>,

    /// IP address of the DNS resolver to use.
    #[arg(long, value_name = "IP")]
    pub dns_resolver: Option<String>,

    /// Enable the real-time metrics display.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub live_metrics: bool,
}

impl Provider for Cli {
    fn metadata(&self) -> Metadata {
        Metadata::named("Command-Line Arguments")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        let mut dict = Dict::new();

        if let Some(window) = self.dedup_window {
            dict.insert(
                "deduplication.cache_ttl_seconds".into(),
                Value::from(window),
            );
        }

        if let Some(rate) = self.sample_rate {
            dict.insert("network.sample_rate".into(), Value::from(rate));
        }

        if let Some(timeout) = self.dns_timeout_ms {
            // TODO: This maps to only one of the two retry backoffs. Re-evaluate.
            dict.insert(
                "dns.retry_config.standard_initial_backoff_ms".into(),
                Value::from(timeout),
            );
        }
        
        // TODO: The `dns_resolver` argument is not yet supported in the config struct.

        if self.live_metrics {
            dict.insert("log_metrics".into(), Value::from(true));
        }

        let mut map = Map::new();
        map.insert(Profile::Default, dict);
        Ok(map)
    }
}