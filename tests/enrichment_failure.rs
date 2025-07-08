//! Integration test for enrichment failure handling.

use anyhow::Result;
use std::{sync::Arc, time::Duration};
use tempfile::NamedTempFile;

mod helpers;
use helpers::{
    app::TestAppBuilder,
    fake_enrichment::FakeEnrichmentProvider,
    mock_ws::MockWebSocket,
    test_metrics::TestMetrics,
};

#[tokio::test]
async fn test_enrichment_failure_increments_failure_metric() -> Result<()> {
    // 1. Initialize logger and a test metrics recorder
    let _ = env_logger::builder().is_test(true).try_init();
    let metrics_recorder = TestMetrics::new();
    metrics::set_global_recorder(metrics_recorder.clone()).unwrap();

    // 2. Setup a mock websocket to send a domain
    let mock_ws = MockWebSocket::new();

    // 3. Setup a failable enrichment provider
    let fake_enrichment = Arc::new(FakeEnrichmentProvider::new());
    fake_enrichment.set_fail(true);

    // 4. Setup a pattern file to match the domain
    let mut pattern_file = NamedTempFile::new()?;
    std::io::Write::write_all(&mut pattern_file, b"\\.com$")?;
    let pattern_path = pattern_file.path().to_path_buf();

    // 5. Build the app with all our test doubles
    let mut app_builder = TestAppBuilder::new()
        .with_pattern_files(vec![pattern_path])
        .with_dns_resolver({
            let resolver = certwatch::dns::test_utils::FakeDnsResolver::new();
            resolver.add_success_response("test.com", {
                let mut dns_info = certwatch::core::DnsInfo::default();
                dns_info.a_records.push("1.1.1.1".parse().unwrap());
                dns_info
            });
            Arc::new(resolver)
        })
        .with_enrichment_provider(fake_enrichment);
    let asn_file = NamedTempFile::new()?;
    app_builder.config.enrichment.asn_tsv_path = Some(asn_file.path().to_path_buf());
    let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;

    // 6. Send a matching domain through the websocket
    mock_ws.add_domain_message("test.com");

    // 7. Give the app a moment to process the domain and fail
    metrics_recorder
        .wait_for_counter("cert_processing_failures", 1, Duration::from_secs(5))
        .await;

    // 8. Assert that the failure counter was incremented
    assert_eq!(
        metrics_recorder.get_counter("cert_processing_failures"),
        1,
        "The failure counter should have been incremented"
    );
    assert_eq!(
        metrics_recorder.get_counter("cert_processing_successes"),
        0,
        "The success counter should not have been incremented"
    );

    // 9. Shut down the app
    mock_ws.close();
    app.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}