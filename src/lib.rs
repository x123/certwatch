pub mod services;
/// CertWatch - A high-performance certificate transparency log monitor
///
/// This library provides the core functionality for monitoring certificate
/// transparency logs and detecting suspicious domain registrations.
pub mod notification;

use chrono::Utc;
pub mod utils;
use std::sync::Arc;
use metrics;

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

use std::time::Instant;

pub async fn build_alert(
    domain: String,
    source_tag: Vec<String>,
    resolved_after_nxdomain: bool,
    dns_info: DnsInfo,
    enrichment_provider: Option<Arc<dyn EnrichmentProvider>>,
    processing_start_time: Option<Instant>,
) -> Result<Alert> {
    let enrichment_data = if let Some(provider) = enrichment_provider {
        let all_ips: Vec<_> = dns_info
            .a_records
            .iter()
            .chain(dns_info.aaaa_records.iter())
            .cloned()
            .collect();

        let enrichment_data_futures = all_ips.into_iter().map(|ip| provider.enrich(ip));
        let start = std::time::Instant::now();
        let result = futures::future::try_join_all(enrichment_data_futures).await?;
        let duration = start.elapsed().as_secs_f64();
        metrics::histogram!("enrichment_duration_seconds").record(duration);
        result
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
        processing_start_time,
    })
}

pub mod app;


