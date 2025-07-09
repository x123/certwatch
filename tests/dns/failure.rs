//! Integration tests for DNS failure handling.

use anyhow::Result;
use certwatch::{
    dns::{DnsRetryConfig, test_utils::FakeDnsResolver, DnsHealthMonitor, DnsResolutionManager},
    core::DnsInfo,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[path = "../helpers/mod.rs"]
mod helpers;

#[tokio::test]
async fn test_nxdomain_retry_logic() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Arrange
    let fake_resolver = Arc::new(FakeDnsResolver::new());
    let retry_config = DnsRetryConfig {
        nxdomain_retries: 1,
        nxdomain_initial_backoff_ms: 10, // Short delay for testing
        ..Default::default()
    };

    let (_tx, shutdown_rx) = watch::channel(());
    let health_monitor =
        DnsHealthMonitor::new(Default::default(), fake_resolver.clone(), shutdown_rx.clone());

    let (manager, mut resolved_rx) =
        DnsResolutionManager::new(fake_resolver.clone(), retry_config, health_monitor, shutdown_rx);

    // Configure resolver to return NXDOMAIN initially, then a success response
    fake_resolver.add_error_response("newly-active.com", "NXDOMAIN");
    let mut success_dns_info = DnsInfo::default();
    success_dns_info.a_records.push("5.5.5.5".parse().unwrap());
    fake_resolver.add_success_response("newly-active.com", success_dns_info.clone());

    // Act: First call should fail immediately and queue the domain for retry
    let initial_result = manager.resolve_with_retry("newly-active.com", "test-tag").await;

    // Assert: Check that the initial call failed with NXDOMAIN
    assert!(initial_result.is_err());
    assert!(initial_result.unwrap_err().to_string().contains("NXDOMAIN"));
    assert_eq!(fake_resolver.get_call_count("newly-active.com"), 1);

    // Assert: Wait for the retry task to process and check for the resolved domain
    // Use a timeout that is generous for a test but prevents it from hanging forever.
    let resolved_result = tokio::time::timeout(Duration::from_millis(250), resolved_rx.recv()).await;
    assert!(
        resolved_result.is_ok(),
        "Timeout waiting for resolved domain from retry channel"
    );

    let received = resolved_result.unwrap();
    assert!(received.is_some(), "The resolved_rx channel was closed unexpectedly");

    let (domain, tag, dns_info) = received.unwrap();
    assert_eq!(domain, "newly-active.com");
    assert_eq!(tag, "test-tag");
    assert_eq!(dns_info.a_records, success_dns_info.a_records);

    // The resolver should have been called a second time by the retry task
    assert_eq!(fake_resolver.get_call_count("newly-active.com"), 2);

    Ok(())
}