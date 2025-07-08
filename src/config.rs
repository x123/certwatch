//! Configuration management for CertWatch
//!
//! This module defines the main `Config` struct and its sub-structs,
//! responsible for holding all application settings. It uses the `figment`
//! crate to load configuration from a `certwatch.toml` file and merge it
//! with environment variables.

use anyhow::Result;
use crate::{cli::Cli, dns::DnsRetryConfig};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// The main configuration struct for the application.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Config {
    /// The logging level for the application.
    pub log_level: String,
    /// The number of concurrent domain processing tasks.
    pub concurrency: usize,
    /// Configuration for metrics.
    pub metrics: MetricsConfig,
    /// Configuration for performance tuning.
    pub performance: PerformanceConfig,
    /// Configuration for the CertStream network client.
    pub network: NetworkConfig,
    /// Configuration for pattern matching.
    pub matching: MatchingConfig,
    /// Configuration for DNS resolution.
    pub dns: DnsConfig,
    /// Configuration for IP address enrichment.
    pub enrichment: EnrichmentConfig,
    /// Configuration for output and alerting.
    pub output: OutputConfig,
    /// Configuration for alert deduplication.
    pub deduplication: DeduplicationConfig,
    // Note: The `notifications` field has been removed.
    // Slack notification settings are now part of `OutputConfig`.
}

/// Configuration for performance tuning.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PerformanceConfig {
    /// The capacity of the central domain queue.
    pub queue_capacity: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 100_000,
        }
    }
}

/// Configuration for the CertStream network client.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct NetworkConfig {
    /// The URL of the CertStream WebSocket server.
    pub certstream_url: String,
    /// The percentage of domains to process (0.0 to 1.0).
    pub sample_rate: f64,
    /// Whether to accept invalid TLS certificates (for testing).
    pub allow_invalid_certs: bool,
}

/// Configuration for pattern matching.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MatchingConfig {
    /// A list of file paths containing regex patterns.
    pub pattern_files: Vec<PathBuf>,
}

/// Configuration for DNS resolution.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct DnsConfig {
    /// The IP address of the DNS resolver to use (e.g., "8.8.8.8:53").
    /// If not set, the system's default resolver will be used.
    pub resolver: Option<String>,
    /// The timeout in milliseconds for a single DNS query.
    pub timeout_ms: Option<u64>,
    /// DNS retry and backoff settings.
    #[serde(flatten)]
    pub retry_config: DnsRetryConfig,
    /// Configuration for the DNS health monitor.
    pub health: DnsHealthConfig,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            resolver: None,
            timeout_ms: Some(5000),
            retry_config: DnsRetryConfig::default(),
            health: DnsHealthConfig::default(),
        }
    }
}

/// Configuration for the DNS health monitor.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct DnsHealthConfig {
    /// The failure rate threshold to trigger the unhealthy state (e.g., 0.95 for 95%).
    pub failure_threshold: f64,
    /// The time window in seconds to consider for the failure rate calculation.
    pub window_seconds: u64,
    /// A known-good domain to resolve to check for recovery.
    pub recovery_check_domain: String,
    /// The interval in seconds between recovery checks when the system is unhealthy.
    pub recovery_check_interval_seconds: u64,
}

impl Default for DnsHealthConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 0.95,
            window_seconds: 120,
            recovery_check_domain: "google.com".to_string(),
            recovery_check_interval_seconds: 10,
        }
    }
}

/// Configuration for metrics.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MetricsConfig {
    /// Log metrics to the console periodically.
    #[serde(default)]
    pub log_metrics: bool,
    /// The interval in seconds for logging aggregated metrics.
    pub log_aggregation_seconds: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            log_metrics: false,
            log_aggregation_seconds: 10,
        }
    }
}

/// Configuration for IP address enrichment.
/// Configuration for IP address enrichment.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct EnrichmentConfig {
    /// Path to the TSV ASN database file.
    pub asn_tsv_path: Option<PathBuf>,
}

/// The format for stdout output.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum OutputFormat {
    Json,
    PlainText,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Json => write!(f, "JSON"),
            OutputFormat::PlainText => write!(f, "PlainText"),
        }
    }
}

/// Configuration for output and alerting.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct OutputConfig {
    /// The format to use for stdout output.
    pub format: OutputFormat,
    /// Configuration for Slack alerts.
    pub slack: Option<SlackConfig>,
}

/// Configuration for Slack alerts.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SlackConfig {
    /// Enables or disables Slack notifications.
    #[serde(default)]
    pub enabled: bool,
    /// The Slack incoming webhook URL.
    pub webhook_url: String,
    /// The maximum number of alerts to batch together before sending.
    #[serde(default = "default_slack_batch_size")]
    pub batch_size: usize,
    /// The maximum time in seconds to wait before sending a batch, even if it's not full.
    #[serde(default = "default_slack_batch_timeout")]
    pub batch_timeout_seconds: u64,
}

fn default_slack_batch_size() -> usize {
    50
}

fn default_slack_batch_timeout() -> u64 {
    300
}

/// Configuration for alert deduplication.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct DeduplicationConfig {
    /// The size of the deduplication cache.
    pub cache_size: usize,
    /// The time-to-live for cache entries in seconds.
    pub cache_ttl_seconds: u64,
}

impl Config {
    /// Loads the application configuration.
    ///
    /// This function builds the final configuration by layering different sources
    /// in the following order of precedence (from lowest to highest):
    /// 1. Default values.
    /// 2. Configuration file (`certwatch.toml` or specified by `--config`).
    /// 3. Environment variables (prefixed with `CERTWATCH_`).
    /// 4. Command-line arguments.
    pub fn load(cli: &Cli) -> Result<Self> {
        let config_path = cli.config.clone().unwrap_or_else(|| "certwatch.toml".into());

        let mut figment = Figment::new()
            .merge(Serialized::defaults(Config::default()));

        if config_path.exists() {
            figment = figment.merge(Toml::file(&config_path));
        }

        let figment = figment.merge(Env::prefixed("CERTWATCH_")).merge(cli.clone());

        let config: Config = figment.extract()?;
        Ok(config)
    }
}

// Provide a default implementation for tests and easy setup.
impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            concurrency: num_cpus::get(),
            metrics: MetricsConfig::default(),
            performance: PerformanceConfig::default(),
            network: NetworkConfig {
                certstream_url: "wss://certstream.calidog.io".to_string(),
                sample_rate: 1.0,
                allow_invalid_certs: false,
            },
            matching: MatchingConfig {
                pattern_files: vec![],
            },
            dns: DnsConfig::default(),
            enrichment: EnrichmentConfig {
                asn_tsv_path: None,
            },
            output: OutputConfig {
                format: OutputFormat::PlainText,
                slack: None,
            },
            deduplication: DeduplicationConfig {
                cache_size: 100_000,
                cache_ttl_seconds: 3600,
            },
        }
    }
}
