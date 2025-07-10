use certwatch::{core::DnsInfo, dns::DnsError};
use std::{sync::Arc, time::Duration};

#[path = "../helpers/mod.rs"]
mod helpers;
use certwatch::internal_metrics::Metrics;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver};

#[tokio::test]
async fn test_app_startup_succeeds_with_healthy_resolver() {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_resolver = Arc::new(MockDnsResolver::new(metrics));
    mock_resolver.add_response("google.com", Ok(DnsInfo::default()));

    let (mut test_app, app_future) = TestAppBuilder::new()
        .with_dns_resolver(mock_resolver)
        .build()
        .await
        .unwrap();

    let app_handle = tokio::spawn(app_future);
    test_app.app_handle = Some(app_handle);

    // Immediately shut down to just test the startup sequence.
    let result = test_app.shutdown(Duration::from_secs(5)).await;

    assert!(
        result.is_ok(),
        "App should start and shut down cleanly with a healthy resolver"
    );
}

#[tokio::test]
async fn test_app_startup_fails_with_unhealthy_resolver() {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_resolver = Arc::new(MockDnsResolver::new(metrics));
    mock_resolver.add_response(
        "google.com",
        Err(DnsError::Resolution("Timeout".to_string())),
    );

    let result = TestAppBuilder::new()
        .with_dns_resolver(mock_resolver)
        .build()
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