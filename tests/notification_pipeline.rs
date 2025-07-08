//! Unit test for the notification pipeline.

use certwatch::{
    core::{Alert, DnsInfo},
    notification::logging_subscriber,
};
use tokio::sync::broadcast;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_logging_subscriber_receives_alert() {
    // 1. Setup a broadcast channel for alerts
    let (alert_tx, alert_rx) = broadcast::channel(16);

    // 2. Spawn the LoggingSubscriber
    logging_subscriber::spawn(alert_rx, None);

    // 3. Create and send a dummy alert
    let test_alert = Alert {
        timestamp: "2025-07-08T12:00:00Z".to_string(),
        domain: "unit-test.com".to_string(),
        source_tag: "test".to_string(),
        resolved_after_nxdomain: false,
        dns: DnsInfo::default(),
        enrichment: vec![],
    };
    alert_tx.send(test_alert).unwrap();

    // 4. Give the subscriber a moment to process the alert
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 5. Assert that the log output contains the expected messages
    assert!(logs_contain("Received alert via notification channel"));
    assert!(logs_contain("domain: \"unit-test.com\""));
}