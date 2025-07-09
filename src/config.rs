//! Configuration management for CertWatch
//!
//! This module defines the main `Config` struct and its sub-structs,
//! responsible for holding all application settings. It uses the `figment`
//! crate to load configuration from a `certwatch.toml` file.

use crate::dns::DnsRetryConfig;
use clap::Parser;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Command-line arguments for the application.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the configuration file.
    #[arg(short, long, value_name = "FILE", default_value = "certwatch.toml")]
    pub config_file: PathBuf,

    /// Run in test mode, exiting after successful startup checks.
    #[arg(long)]
    pub test_mode: bool,
}

/// The main configuration struct for the application.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(skip)] // test_mode is a runtime flag, not a config value.
    pub test_mode: bool,
    #[serde(default)]
    pub core: CoreConfig,
    pub metrics: MetricsConfig,
    pub performance: PerformanceConfig,
    pub network: NetworkConfig,
    pub rules: RulesConfig,
    pub dns: DnsConfig,
    pub enrichment: EnrichmentConfig,
    pub output: OutputConfig,
    pub deduplication: DeduplicationConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            test_mode: false,
            core: CoreConfig::default(),
            metrics: MetricsConfig::default(),
            performance: PerformanceConfig::default(),
            network: NetworkConfig::default(),
            rules: RulesConfig::default(),
            dns: DnsConfig::default(),
            enrichment: EnrichmentConfig::default(),
            output: OutputConfig::default(),
            deduplication: DeduplicationConfig::default(),
        }
    }
}

/// Configuration for core application settings.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct CoreConfig {
    pub log_level: String,
    pub concurrency: usize,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            concurrency: num_cpus::get(),
        }
    }
}

/// Configuration for performance tuning.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct PerformanceConfig {
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
#[serde(default)]
pub struct NetworkConfig {
    pub certstream_url: String,
    pub sample_rate: f64,
    pub allow_invalid_certs: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            certstream_url: "wss://certstream.calidog.io".to_string(),
            sample_rate: 1.0,
            allow_invalid_certs: false,
        }
    }
}

/// Configuration for advanced rule-based filtering.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct RulesConfig {
    pub rule_files: Option<Vec<PathBuf>>,
}

/// Configuration for DNS resolution.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct DnsConfig {
    pub resolver: Option<String>,
    pub timeout_ms: u64,
    #[serde(flatten)]
    pub retry_config: DnsRetryConfig,
    pub health: DnsHealthConfig,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            resolver: None,
            timeout_ms: 5000,
            retry_config: DnsRetryConfig::default(),
            health: DnsHealthConfig::default(),
        }
    }
}

/// Configuration for the DNS health monitor.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct DnsHealthConfig {
    pub failure_threshold: f64,
    pub window_seconds: u64,
    pub recovery_check_domain: String,
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
#[serde(default)]
pub struct MetricsConfig {
    pub log_metrics: bool,
    pub log_aggregation_seconds: u64,
    pub prometheus_enabled: bool,
    pub prometheus_listen_address: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            log_metrics: false,
            log_aggregation_seconds: 10,
            prometheus_enabled: false,
            prometheus_listen_address: "127.0.0.1:9090".to_string(),
        }
    }
}

/// Configuration for IP address enrichment.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct EnrichmentConfig {
    pub asn_tsv_path: Option<PathBuf>,
}

/// The format for stdout output.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub enum OutputFormat {
    #[default]
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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct OutputConfig {
    pub format: Option<OutputFormat>,
    pub slack: Option<SlackConfig>,
}

/// Configuration for Slack alerts.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct SlackConfig {
    pub enabled: Option<bool>,
    pub webhook_url: Option<String>,
    pub batch_size: Option<usize>,
    pub batch_timeout_seconds: Option<u64>,
}

/// Configuration for alert deduplication.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(default)]
pub struct DeduplicationConfig {
    pub cache_size: usize,
    pub cache_ttl_seconds: u64,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            cache_size: 100_000,
            cache_ttl_seconds: 3600,
        }
    }
}

impl Config {
    /// Loads the application configuration by parsing command-line arguments.
    pub fn load() -> anyhow::Result<Self> {
        let cli_args = Cli::parse();
        Self::load_from_cli(cli_args)
    }

    /// Loads the application configuration from a given `Cli` struct.
    /// This is the core logic, made public for testing purposes.
    pub fn load_from_cli(cli_args: Cli) -> anyhow::Result<Self> {
        let config_path = &cli_args.config_file;

        // Check if the config file exists.
        if !config_path.exists() {
            anyhow::bail!("Config file not found at specified path: {:?}", config_path);
        }

        // Build the final config by loading the TOML file over the defaults.
        let figment = Figment::new()
            .merge(figment::providers::Serialized::defaults(Config::default()))
            .merge(Toml::file(config_path));

        // Extract the configuration.
        let mut config: Config = figment
            .extract()
            .map_err(|e| anyhow::anyhow!("Configuration loading error: {}", e))?;

        // Set the runtime test_mode flag from the CLI.
        config.test_mode = cli_args.test_mode;

        Ok(config)
    }
}
