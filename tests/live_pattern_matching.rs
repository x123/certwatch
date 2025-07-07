//! Integration test for the pattern matching engine.
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;

mod helpers;
use helpers::{
    app::TestAppBuilder,
    mock_output::FailableMockOutput,
    mock_ws::MockWebSocket,
};

/// Test that the application correctly matches a domain from the WebSocket
/// against the configured patterns and sends an alert.
#[tokio::test]
async fn test_pattern_matching_sends_alert() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Create a mock websocket and a mock output to capture alerts.
    let mock_ws = MockWebSocket::new();
    let mock_output = Arc::new(FailableMockOutput::new());

    // 2. Create a temporary pattern file.
    let mut pattern_file = NamedTempFile::new()?;
    std::io::Write::write_all(&mut pattern_file, b"github\\.com\tgit-domains")?;

    // 3. Build the application with our mocks and the real pattern file.
    // We also need to provide a fake enrichment provider and DNS resolver.
    let fake_dns_resolver = certwatch::dns::test_utils::FakeDnsResolver::new();
    fake_dns_resolver.add_success_response("www.github.com", {
        let mut dns_info = certwatch::core::DnsInfo::default();
        dns_info.a_records.push("1.1.1.1".parse().unwrap());
        dns_info
    });

    let mut app_builder = TestAppBuilder::new()
        .with_outputs(vec![mock_output.clone()])
        .with_pattern_files(vec![pattern_file.path().to_path_buf()])
        .with_enrichment_provider(Arc::new(helpers::fake_enrichment::FakeEnrichmentProvider::new()))
        .with_dns_resolver(Arc::new(fake_dns_resolver));

    // The app requires an ASN database file to exist, even if it's empty.
    let asn_file = NamedTempFile::new()?;
    app_builder.config.enrichment.asn_tsv_path = Some(asn_file.path().to_path_buf());

    let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;

    // 4. Send a matching domain through the mock websocket.
    println!("[DEBUG] Sending domain 'www.github.com' to mock websocket...");
    mock_ws.add_domain_message("www.github.com");

    // 5. Wait a moment for the app to process the domain.
    println!("[DEBUG] Waiting for 2 seconds for processing...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 6. Assert that our mock output received exactly one alert.
    let alerts = mock_output.alerts.lock().unwrap();
    println!("[DEBUG] Found {} alerts in mock output.", alerts.len());
    assert_eq!(
        alerts.len(),
        1,
        "An alert should have been sent for the matching domain"
    );

    // 7. Shutdown the app.
    mock_ws.close();
    app.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
