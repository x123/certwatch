//! Configuration management for CertWatch
//!
//! This module defines the main `Config` struct and its sub-structs,
//! responsible for holding all application settings. It uses the `figment`
//! crate to load configuration from a `certwatch.toml` file and merge it
//! with environment variables.

use anyhow::Result;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use serde::Deserialize;
use std::path::PathBuf;

use crate::dns::DnsRetryConfig;

/// The main configuration struct for the application.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// The logging level for the application.
    pub log_level: String,
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
}

/// Configuration for the CertStream network client.
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    /// The URL of the CertStream WebSocket server.
    pub certstream_url: String,
    /// The percentage of domains to process (0.0 to 1.0).
    pub sample_rate: f64,
    /// Whether to accept invalid TLS certificates (for testing).
    pub allow_invalid_certs: bool,
}

/// Configuration for pattern matching.
#[derive(Debug, Deserialize, Clone)]
pub struct MatchingConfig {
    /// A list of file paths containing regex patterns.
    pub pattern_files: Vec<PathBuf>,
}

/// Configuration for DNS resolution.
#[derive(Debug, Deserialize, Clone)]
pub struct DnsConfig {
    /// The number of concurrent DNS resolvers to use.
    pub resolver_pool_size: usize,
    /// DNS retry and backoff settings.
    #[serde(flatten)]
    pub retry_config: DnsRetryConfig,
}

/// Configuration for IP address enrichment.
#[derive(Debug, Deserialize, Clone)]
pub enum AsnProvider {
    Maxmind,
    Tsv,
}

/// Configuration for IP address enrichment.
#[derive(Debug, Deserialize, Clone)]
pub struct EnrichmentConfig {
    /// The ASN provider to use.
    pub asn_provider: AsnProvider,
    /// Path to the MaxMind GeoLite2-ASN database file (if using Maxmind).
    pub asn_db_path: Option<PathBuf>,
    /// Path to the TSV ASN database file (if using Tsv).
    pub asn_tsv_path: Option<PathBuf>,
    /// Path to the MaxMind GeoLite2-Country database file.
    pub geoip_db_path: PathBuf,
}

/// The format for stdout output.
#[derive(Debug, Deserialize, Clone)]
pub enum OutputFormat {
    Json,
    PlainText,
}

/// Configuration for output and alerting.
#[derive(Debug, Deserialize, Clone)]
pub struct OutputConfig {
    /// The format to use for stdout output.
    pub format: OutputFormat,
    /// Configuration for Slack alerts.
    pub slack: Option<SlackConfig>,
}

/// Configuration for Slack alerts.
#[derive(Debug, Deserialize, Clone)]
pub struct SlackConfig {
    /// The Slack incoming webhook URL.
    pub webhook_url: String,
}

/// Configuration for alert deduplication.
#[derive(Debug, Deserialize, Clone)]
pub struct DeduplicationConfig {
    /// The size of the deduplication cache.
    pub cache_size: usize,
    /// The time-to-live for cache entries in seconds.
    pub cache_ttl_seconds: u64,
}

impl Config {
    /// Loads the application configuration from the specified file.
    ///
    /// # Arguments
    /// * `config_path` - The path to the TOML configuration file.
    pub fn load(config_path: &str) -> Result<Self> {
        let config: Config = Figment::new()
            .merge(Toml::file(config_path))
            .extract()?;
        Ok(config)
    }
}

// Provide a default implementation for tests and easy setup.
impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            network: NetworkConfig {
                certstream_url: "wss://certstream.calidog.io".to_string(),
                sample_rate: 1.0,
                allow_invalid_certs: false,
            },
            matching: MatchingConfig {
                pattern_files: vec![],
            },
            dns: DnsConfig {
                resolver_pool_size: 10,
                retry_config: DnsRetryConfig::default(),
            },
            enrichment: EnrichmentConfig {
                asn_provider: AsnProvider::Maxmind,
                asn_db_path: Some("data/GeoLite2-ASN.mmdb".into()),
                asn_tsv_path: None,
                geoip_db_path: "data/GeoLite2-Country.mmdb".into(),
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
