//! Unit test for enrichment failure handling.

use anyhow::Result;
use certwatch::{
    app::process_domain,
    core::{DnsInfo, PatternMatcher},
    dns::{test_utils::FakeDnsResolver, DnsHealthMonitor, DnsResolutionManager},
    enrichment::fake::FakeEnrichmentProvider,
};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

// A mock pattern matcher for testing purposes.
struct MockPatternMatcher;
#[async_trait::async_trait]
impl PatternMatcher for MockPatternMatcher {
    async fn match_domain(&self, _domain: &str) -> Option<String> {
        Some("test-source".to_string())
    }
}

#[tokio::test]
async fn test_process_domain_propagates_enrichment_error() -> Result<()> {
    // 1. Arrange
    let pattern_matcher = Arc::new(MockPatternMatcher);
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let dns_resolver = Arc::new(FakeDnsResolver::new());
    let mut dns_info = DnsInfo::default();
    let error_ip = "1.2.3.4".parse().unwrap();
    dns_info.a_records.push(error_ip);
    dns_resolver.add_success_response("example.com", dns_info);

    let (dns_manager, _) = DnsResolutionManager::new(
        dns_resolver.clone(),
        Default::default(),
        DnsHealthMonitor::new(Default::default(), dns_resolver, shutdown_rx.clone()),
        shutdown_rx,
    );
    let dns_manager = Arc::new(dns_manager);

    // Configure the enrichment provider to fail for the specific IP
    let enrichment_provider = FakeEnrichmentProvider::new();
    enrichment_provider.add_error(error_ip, "Enrichment database offline");
    let enrichment_provider = Arc::new(enrichment_provider);

    let (alerts_tx, _) = mpsc::channel(1);

    // 2. Act
    let result = process_domain(
        "example.com".to_string(),
        0, // worker_id
        pattern_matcher,
        dns_manager,
        enrichment_provider,
        alerts_tx,
    )
    .await;

    // 3. Assert
    assert!(result.is_err(), "Expected process_domain to return an error");
    let err_string = result.unwrap_err().to_string();
    assert!(
        err_string.contains("Failed to build alert"),
        "Error message did not contain expected text"
    );

    // ensure shutdown channel is used
    drop(shutdown_tx);
    Ok(())
}