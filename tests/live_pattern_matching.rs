//! Unit test for the pattern matching engine.
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
async fn test_process_domain_sends_alert_on_match() -> Result<()> {
    // 1. Arrange
    let pattern_matcher = Arc::new(MockPatternMatcher);
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let dns_resolver = Arc::new(FakeDnsResolver::new());
    dns_resolver.add_success_response("matching.com", DnsInfo::default());

    let (dns_manager, _) = DnsResolutionManager::new(
        dns_resolver.clone(),
        Default::default(),
        DnsHealthMonitor::new(Default::default(), dns_resolver, shutdown_rx.clone()),
        shutdown_rx,
    );
    let dns_manager = Arc::new(dns_manager);

    let enrichment_provider = Arc::new(FakeEnrichmentProvider::new());
    let (alerts_tx, mut alerts_rx) = mpsc::channel(1);

    // 2. Act
    let result = process_domain(
        "matching.com".to_string(),
        0, // worker_id
        pattern_matcher,
        dns_manager,
        enrichment_provider,
        alerts_tx,
    )
    .await;

    // 3. Assert
    assert!(result.is_ok(), "process_domain should succeed");

    let alert = alerts_rx.recv().await;
    assert!(alert.is_some(), "An alert should have been sent");
    let alert = alert.unwrap();
    assert_eq!(alert.domain, "matching.com");
    assert_eq!(alert.source_tag, "test-source");

    // ensure shutdown channel is used
    drop(shutdown_tx);
    Ok(())
}
