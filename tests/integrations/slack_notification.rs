//! Integration tests for Slack notifications.

use crate::helpers::{app::TestAppBuilder, mock_slack::MockSlackClient};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_slack_alert_aggregation() -> Result<()> {
    // 1. Create a MockSlackClient
    let mock_slack_client = Arc::new(MockSlackClient::new());

    // 2. Build the TestApp with the mock client and a rule
    let test_app_builder = TestAppBuilder::new()
        .with_slack_notification(mock_slack_client.clone(), "#test")
        .with_config_modifier(|config| {
            config.notification.aggregation_time = Duration::from_millis(100);
        });

    let rules = r#"
rules:
  - name: "Catch All"
    pattern: ".*"
    action: "alert"
"#;
    let (mut app, app_handle) = test_app_builder.with_rules(rules).await.build().await?;
    let _app_join_handle = tokio::spawn(app_handle);

    // 3. Send domains to the app
    let domains = vec![
        "test.example.com",
        "sub.test.example.com",
        "another.example.com",
        "sub.another.example.com",
        "sub.sub.another.example.com",
    ];

    for domain in domains {
        app.domains_tx.send(domain.to_string()).await?;
    }

    // Give the app time to process and aggregate the alerts
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 4. Assert the aggregated alert is correct
    let sent_batches = mock_slack_client.get_sent_batches().await;
    assert_eq!(sent_batches.len(), 1, "Expected exactly one batch of alerts");

    let batch = &sent_batches[0];
    assert_eq!(batch.len(), 2, "Expected two aggregated alerts in the batch");

    // Check for example.com aggregation
    let example_alert = batch
        .iter()
        .find(|a| a.domain == "test.example.com")
        .expect("Did not find alert for test.example.com");
    assert_eq!(
        example_alert.deduplicated_count, 1,
        "Expected 1 deduplicated domain for example.com"
    );

    // Check for another.com aggregation
    let another_alert = batch
        .iter()
        .find(|a| a.domain == "another.example.com")
        .expect("Did not find alert for another.example.com");
    assert_eq!(
        another_alert.deduplicated_count, 2,
        "Expected 2 deduplicated domains for another.com"
    );

    app.shutdown(Duration::from_secs(1)).await?;

    Ok(())
}