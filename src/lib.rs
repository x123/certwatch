//! CertWatch - A high-performance certificate transparency log monitor
//!
//! This library provides the core functionality for monitoring certificate
//! transparency logs and detecting suspicious domain registrations.

use chrono::Utc;
use log::error;
pub mod utils;
use std::sync::Arc;

pub mod cli;
pub mod config;
pub mod core;
pub mod deduplication;
pub mod dns;
pub mod enrichment;
pub mod matching;
pub mod metrics;
pub mod network;
pub mod outputs;

// Re-export core types for convenience
pub use core::*;

/// Helper function to build an alert
pub async fn build_alert(
    domain: String,
    source_tag: String,
    resolved_after_nxdomain: bool,
    dns_info: DnsInfo,
    enrichment_provider: Arc<dyn EnrichmentProvider>,
) -> Alert {
    let mut enrichment_data = Vec::new();
    let all_ips: Vec<_> = dns_info
        .a_records
        .iter()
        .chain(dns_info.aaaa_records.iter())
        .cloned()
        .collect();

    for ip in all_ips {
        match enrichment_provider.enrich(ip).await {
            Ok(info) => enrichment_data.push(info),
            Err(e) => error!("Failed to enrich IP {}: {}", ip, e),
        }
    }

    Alert {
        timestamp: Utc::now().to_rfc3339(),
        domain,
        source_tag,
        resolved_after_nxdomain,
        dns: dns_info,
        enrichment: enrichment_data,
    }
}
