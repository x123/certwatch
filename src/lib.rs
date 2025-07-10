pub mod services;
/// CertWatch - A high-performance certificate transparency log monitor
///
/// This library provides the core functionality for monitoring certificate
/// transparency logs and detecting suspicious domain registrations.
pub mod notification;

use chrono::Utc;
pub mod utils;
use std::sync::Arc;

pub mod config;
pub mod core;
pub mod deduplication;
pub mod dns;
pub mod enrichment;
pub mod formatting;
pub mod network;
pub mod internal_metrics;
pub mod outputs;
pub mod rules;
pub mod types;

// Re-export core types for convenience
pub use core::*;

/// Helper function to build an alert
use anyhow::Result;

pub async fn build_alert(
    domain: String,
    source_tag: String,
    resolved_after_nxdomain: bool,
    dns_info: DnsInfo,
    enrichment_provider: Option<Arc<dyn EnrichmentProvider>>,
) -> Result<Alert> {
    let enrichment_data = if let Some(provider) = enrichment_provider {
        let all_ips: Vec<_> = dns_info
            .a_records
            .iter()
            .chain(dns_info.aaaa_records.iter())
            .cloned()
            .collect();

        let enrichment_data_futures = all_ips.into_iter().map(|ip| provider.enrich(ip));
        futures::future::try_join_all(enrichment_data_futures).await?
    } else {
        Vec::new()
    };

    Ok(Alert {
        timestamp: Utc::now().to_rfc3339(),
        domain,
        source_tag,
        resolved_after_nxdomain,
        dns: dns_info,
        enrichment: enrichment_data,
    })
}

pub mod app;

