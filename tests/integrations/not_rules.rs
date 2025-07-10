#[path = "../helpers/mod.rs"]
mod helpers;

use helpers::app::TestAppBuilder;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::test]
async fn test_not_asns_rule_does_not_match_on_enrichment_failure() {
    // Arrange
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::new("info,certwatch::network=off"))
        .init();

    let (alerts_tx, mut alerts_rx) = broadcast::channel(100);

    // 1. Configure a rule that should only match if enrichment is successful
    let rules = r#"
rules:
  - name: "Stage 1 Pass-Through"
    domain_regex: ".*"
  - name: "Non-Google .com"
    all:
      - domain_regex: "\\.com$"
      - not_asns: [15169] # Google LLC
"#;

    // 2. Build the test application, configuring the enrichment provider to fail
    let (mut app, app_future) = TestAppBuilder::new()
        .with_alert_tx(alerts_tx)
        .with_failing_enrichment(true)
        .with_rules(rules)
        .await
        .build()
        .await
        .unwrap();

    // 4. Run the app in the background
    app.app_handle = Some(tokio::spawn(app_future));

    // 5. Act: Send a domain that matches the regex part of the rule
    app.domains_tx
        .send("example.com".to_string())
        .await
        .unwrap();

    // 6. Assert: Check that NO alert is received
    // With the buggy logic, the enrichment failure would cause the `not_asns`
    // to incorrectly return `true`, and an alert would be sent.
    // The correct logic should not produce an alert.
    let recv_result = tokio::time::timeout(Duration::from_secs(5), alerts_rx.recv()).await;

    assert!(
        recv_result.is_err(),
        "Expected no alert to be sent, but one was received."
    );

    // Cleanup
    app.shutdown(Duration::from_secs(1)).await.unwrap();
}