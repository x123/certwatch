//! Integration test for output failure handling.

use anyhow::Result;
use std::{sync::Arc, time::Duration};
use tempfile::NamedTempFile;
use tokio::time::timeout;

mod helpers;
use helpers::{
    app::TestAppBuilder,
    fake_dns::FakeDnsResolver,
    mock_output::FailableMockOutput,
    mock_ws::MockWebSocket,
};

#[tokio::test]
async fn test_output_failure_does_not_crash_app() -> Result<()> {
    // 1. Initialize logger to see output
    let _ = env_logger::builder().is_test(true).try_init();

    // 2. Setup a mock websocket to send a domain
    let mock_ws = MockWebSocket::new();

    // 3. Setup a failable mock output
    let mock_output = Arc::new(FailableMockOutput::new());

    // 4. Setup a pattern file and a dummy enrichment file
    let mut pattern_file = NamedTempFile::new()?;
    std::io::Write::write_all(&mut pattern_file, b"\\.com$")?;
    let pattern_path = pattern_file.path().to_path_buf();

    let asn_file = NamedTempFile::new()?;
    let asn_path = asn_file.path().to_path_buf();

    // 5. Build the app with the mock websocket and failable output
    let mut app_builder = TestAppBuilder::new()
        .with_outputs(vec![mock_output.clone()])
        .with_pattern_files(vec![pattern_path])
        .with_dns_resolver(Arc::new(FakeDnsResolver::new()));
    app_builder.config.enrichment.asn_tsv_path = Some(asn_path);
    let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;

    // 6. Configure the mock output to fail
    mock_output.set_fail_on_send(true);

    // 7. Send a matching domain through the websocket
    mock_ws.add_domain_message("test.com");

    // 8. Give the app a moment to process the alert and fail
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 9. Assert that no alerts were successfully sent
    assert!(mock_output.alerts.lock().unwrap().is_empty(), "An alert was sent despite the mock being configured to fail");

    // 10. Configure the mock output to succeed
    mock_output.set_fail_on_send(false);

    // 11. Send another matching domain
    mock_ws.add_domain_message("test2.com");

    // 12. Wait for the alert to be processed
    let alert_check = async {
        loop {
            if !mock_output.alerts.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    timeout(Duration::from_secs(5), alert_check).await?;

    // 13. Assert that the second alert was received
    let alerts = mock_output.alerts.lock().unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].domain, "test2.com");

    // 14. Shut down the app
    mock_ws.close();
    app.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}