use certwatch::{core::DnsInfo, dns::DnsError};
use std::sync::Arc;

#[path = "../helpers/mod.rs"]
mod helpers;
use certwatch::internal_metrics::Metrics;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver, mock_ws::MockWebSocket};

#[tokio::test]
#[ignore]
async fn test_app_startup_succeeds_with_healthy_resolver() {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_resolver = Arc::new(MockDnsResolver::new_with_metrics(metrics));
    mock_resolver.add_response("google.com", Ok(DnsInfo::default()));

    let mock_ws = MockWebSocket::new_silent();

    let rules = r#"
rules:
  - name: "allow google.com"
    domain_regex: "google.com"
"#;

    let test_app = TestAppBuilder::new()
        .with_dns_resolver(mock_resolver)
        .with_websocket(Box::new(mock_ws))
        .with_rules(rules)
        .with_skipped_health_check()
        .start()
        .await
        .unwrap();

    // Immediately shut down to just test the startup sequence.
    let result = test_app.shutdown().await;

    assert!(
        result.is_ok(),
        "App should start and shut down cleanly with a healthy resolver"
    );
}

#[tokio::test]
async fn test_app_startup_fails_with_unhealthy_resolver() {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_resolver = Arc::new(MockDnsResolver::new_with_metrics(metrics));
    mock_resolver.add_response(
        "google.com",
        Err(DnsError::Resolution("Timeout".to_string())),
    );

    let result = TestAppBuilder::new()
        .with_dns_resolver(mock_resolver)
        .start()
        .await;

    assert!(result.is_err(), "App startup should fail");
    if let Err(e) = result {
        let err_str = e.to_string();
        assert!(
            err_str.contains("DNS health check failed"),
            "Error message did not contain expected text: '{}'",
            err_str
        );
    }
}