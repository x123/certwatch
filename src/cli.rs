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
#[derive(Parser, Debug, Clone, Default)]
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

    /// Number of concurrent domain processing tasks.
    #[arg(long, value_name = "COUNT")]
    pub concurrency: Option<usize>,

    /// The capacity of the central domain queue.
    #[arg(long, value_name = "COUNT")]
    pub queue_capacity: Option<usize>,

    /// Timeout for DNS resolution in milliseconds.
    #[arg(long, value_name = "MS")]
    pub dns_timeout_ms: Option<u64>,

    /// IP address of the DNS resolver to use.
    #[arg(long, value_name = "IP")]
    pub dns_resolver: Option<String>,

    /// Periodically log key metrics to the console.
    #[arg(long)]
    pub log_metrics: Option<bool>,

    /// The interval in seconds for logging aggregated metrics.
    #[arg(long, value_name = "SECONDS")]
    pub log_aggregation_seconds: Option<u64>,

    /// Enable the Prometheus exporter endpoint.
    #[arg(long)]
    pub prometheus_enabled: Option<bool>,

    /// The listen address for the Prometheus exporter.
    #[arg(long, value_name = "ADDRESS")]
    pub prometheus_listen_address: Option<String>,

    /// Output alerts in JSON format to stdout, overriding the config file setting.
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    pub json: bool,

    /// Path to the TSV ASN database file.
    #[arg(long, value_name = "FILE")]
    pub enrichment_asn_tsv_path: Option<PathBuf>,

    /// (For testing only) Exit immediately after successful startup.
    #[arg(long, hide = true)]
    pub test_mode: bool,

    /// Enable Slack notifications, overriding the config file setting.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub slack_enabled: bool,

    /// The Slack incoming webhook URL.
    #[arg(long, value_name = "URL")]
    pub slack_webhook_url: Option<String>,

    /// The maximum number of alerts to batch together before sending to Slack.
    #[arg(long, value_name = "SIZE")]
    pub slack_batch_size: Option<usize>,

    /// The maximum time in seconds to wait before sending a Slack batch.
    #[arg(long, value_name = "SECONDS")]
    pub slack_batch_timeout: Option<u64>,
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

        if let Some(concurrency) = self.concurrency {
            dict.insert("concurrency".into(), Value::from(concurrency as u64));
        }

        if let Some(capacity) = self.queue_capacity {
            dict.insert(
                "performance.queue_capacity".into(),
                Value::from(capacity as u64),
            );
        }

        if let Some(timeout) = self.dns_timeout_ms {
            dict.insert("dns.timeout_ms".into(), Value::from(timeout));
        }

        if let Some(resolver) = self.dns_resolver.as_ref() {
            dict.insert("dns.resolver".into(), Value::from(resolver.clone()));
        }

        if let Some(log_metrics) = self.log_metrics {
            dict.insert("metrics.log_metrics".into(), Value::from(log_metrics));
        }

        if let Some(seconds) = self.log_aggregation_seconds {
            dict.insert(
                "metrics.log_aggregation_seconds".into(),
                Value::from(seconds),
            );
        }

        if let Some(enabled) = self.prometheus_enabled {
            dict.insert("metrics.prometheus_enabled".into(), Value::from(enabled));
        }

        if let Some(address) = self.prometheus_listen_address.as_ref() {
            dict.insert(
                "metrics.prometheus_listen_address".into(),
                Value::from(address.clone()),
            );
        }

        if let Some(path) = self.enrichment_asn_tsv_path.as_ref() {
            let mut enrichment_dict = Dict::new();
            enrichment_dict.insert(
                "asn_tsv_path".into(),
                Value::from(path.to_string_lossy().into_owned()),
            );
            dict.insert("enrichment".into(), Value::from(enrichment_dict));
        }

        if self.slack_enabled {
            dict.insert("output.slack.enabled".into(), Value::from(true));
        }

        if let Some(url) = &self.slack_webhook_url {
            dict.insert("output.slack.webhook_url".into(), Value::from(url.clone()));
        }
        if let Some(size) = self.slack_batch_size {
            dict.insert("output.slack.batch_size".into(), Value::from(size as u64));
        }
        if let Some(timeout) = self.slack_batch_timeout {
            dict.insert(
                "output.slack.batch_timeout_seconds".into(),
                Value::from(timeout),
            );
        }

        let mut map = Map::new();
        map.insert(Profile::Default, dict);
        Ok(map)
    }
}