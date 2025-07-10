//! Core domain types and service traits for CertWatch
//!
//! This module defines the fundamental data structures and trait contracts
//! that govern component interactions throughout the application.

use crate::dns::DnsError;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::net::IpAddr;

/// Represents a security alert for a suspicious domain
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Alert {
    /// ISO 8601 timestamp when the alert was generated
    pub timestamp: String,
    /// The suspicious domain name
    pub domain: String,
    /// Tag identifying the rule source (e.g., "phishing", "typosquatting")
    pub source_tag: String,
    /// Flag indicating if this domain was previously NXDOMAIN but now resolves
    pub resolved_after_nxdomain: bool,
    /// DNS resolution information
    pub dns: DnsInfo,
    /// Enrichment data for resolved IPs
    pub enrichment: Vec<EnrichmentInfo>,
}

impl Alert {
    /// Creates a new, minimal alert containing only a domain name.
    /// This is used for pre-enrichment rule evaluation.
    pub fn new_minimal(domain: &str) -> Self {
        Self {
            domain: domain.to_string(),
            ..Default::default()
        }
    }

    /// Returns a flattened list of all IP addresses (IPv4 and IPv6) from the alert.
    pub fn all_ips(&self) -> Vec<IpAddr> {
        self.dns
            .a_records
            .iter()
            .chain(self.dns.aaaa_records.iter())
            .cloned()
            .collect()
    }
}

/// DNS resolution information for a domain
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DnsInfo {
    /// IPv4 addresses from A records
    pub a_records: Vec<IpAddr>,
    /// IPv6 addresses from AAAA records
    pub aaaa_records: Vec<IpAddr>,
    /// Name servers from NS records
    pub ns_records: Vec<String>,
}
impl DnsInfo {
    /// Checks if the DnsInfo struct contains any records.
    pub fn is_empty(&self) -> bool {
        self.a_records.is_empty() && self.aaaa_records.is_empty() && self.ns_records.is_empty()
    }
}


/// Combined enrichment information for an IP address
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnrichmentInfo {
    /// The IP address this information relates to
    pub ip: IpAddr,
    /// ASN and GeoIP information
    #[serde(flatten)]
    pub asn_info: Option<AsnInfo>,
}

impl Default for EnrichmentInfo {
    fn default() -> Self {
        Self {
            ip: std::net::Ipv4Addr::UNSPECIFIED.into(),
            asn_info: None,
        }
    }
}

/// ASN and GeoIP data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AsnInfo {
    /// Autonomous System Number
    pub as_number: u32,
    /// Human-readable name of the AS
    pub as_name: String,
    /// ISO country code where the IP is located
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
}

// =============================================================================
// Service Traits
// =============================================================================

/// Resolves domain names to their DNS records
#[async_trait]
pub trait DnsResolver: Send + Sync {
    /// Resolves a domain to its DNS records (A, AAAA, NS)
    ///
    /// # Arguments
    /// * `domain` - The domain name to resolve
    ///
    /// # Returns
    /// * `Ok(DnsInfo)` with populated records on successful resolution
    /// * `Err` for DNS errors including NXDOMAIN, timeouts, server errors
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError>;
}

/// Provides enrichment data (ASN, GeoIP, etc.) for an IP address
#[async_trait]
pub trait EnrichmentProvider: Send + Sync {
    /// Retrieves all available enrichment data for an IP address
    ///
    /// # Arguments
    /// * `ip` - The IP address to look up
    ///
    /// # Returns
    /// * `Ok(EnrichmentInfo)` containing all data that could be found
    /// * `Err` only if a critical, unrecoverable error occurs
    async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo>;

    /// Returns this trait as an `Any` object, to allow for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Sends alerts to output destinations
#[async_trait]
pub trait Output: Send + Sync {
    /// A unique, descriptive name for the output (e.g., "stdout", "slack").
    /// Used for logging and metrics.
    fn name(&self) -> &str;

    /// Sends an alert to the configured output destination
    ///
    /// # Arguments
    /// * `alert` - The alert to send
    ///
    /// # Returns
    /// * `Ok(())` if the alert was successfully sent
    /// * `Err` if sending failed (network error, formatting error, etc.)
    async fn send_alert(&self, alert: &Alert) -> Result<()>;
}

/// Represents an alert that has been aggregated with other similar alerts.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregatedAlert {
    /// The representative alert for the group.
    pub alert: Alert,
    /// The number of other alerts that were deduplicated into this one.
    pub deduplicated_count: usize,
}
