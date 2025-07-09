use certwatch::{
    app,
    config::Config,
    core::DnsInfo,
    dns::test_utils::FakeDnsResolver,
    enrichment::fake::FakeEnrichmentProvider,
};
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

#[tokio::test]
async fn test_app_startup_succeeds_with_healthy_resolver() {
    let config = Config::default();
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let fake_resolver = Arc::new(FakeDnsResolver::new());
    fake_resolver.add_success_response("google.com", DnsInfo::default());
    let fake_enrichment = Arc::new(FakeEnrichmentProvider::new());

    let (domains_tx, _) = broadcast::channel::<String>(config.performance.queue_capacity);

    let app_handle = tokio::spawn(app::run(
        config,
        shutdown_rx,
        domains_tx,
        Some(vec![]),
        None,
        Some(fake_resolver),
        Some(fake_enrichment),
        None,
    ));

    // Signal shutdown and wait for the app to complete.
    shutdown_tx.send(()).unwrap();
    let result = app_handle.await.unwrap();

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_app_startup_fails_with_unhealthy_resolver() {
    let config = Config::default();
    let (_shutdown_tx, shutdown_rx) = watch::channel(());

    let fake_resolver = Arc::new(FakeDnsResolver::new());
    fake_resolver.add_error_response("google.com", "Timeout");
    let fake_enrichment = Arc::new(FakeEnrichmentProvider::new());

    let (domains_tx, _) = broadcast::channel::<String>(config.performance.queue_capacity);

    let result = app::run(
        config,
        shutdown_rx,
        domains_tx,
        Some(vec![]),
        None,
        Some(fake_resolver),
        Some(fake_enrichment),
        None,
    )
    .await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("DNS health check failed"),
        "Error message did not contain expected text: '{}'",
        err_str
    );
}