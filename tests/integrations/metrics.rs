#[path = "../helpers/mod.rs"]
mod helpers;

use certwatch::core::AggregatedAlert;
use helpers::app::TestAppBuilder;
use std::{sync::{Arc, Mutex}, time::Duration};
use tokio::sync::{mpsc, broadcast};
use async_trait::async_trait;
use certwatch::notification::slack::SlackClientTrait;

#[tokio::test]
async fn alert_queue_time_metric_is_recorded() {
    // A fake Slack client for testing the manager's batching logic.
    #[derive(Clone)]
    struct FakeSlackClient {
        sent_batches: Arc<Mutex<Vec<Vec<AggregatedAlert>>>>,
    }

    impl FakeSlackClient {
        fn new() -> Self {
            Self {
                sent_batches: Arc::new(Mutex::new(Vec::new())),
            }
        }

    }

    #[async_trait]
    impl SlackClientTrait for FakeSlackClient {
        // A fake implementation of send_batch that just records the batch.
        async fn send_batch(&self, alerts: &[AggregatedAlert]) -> anyhow::Result<()> {
            let mut batches = self.sent_batches.lock().unwrap();
            batches.push(alerts.to_vec());
            Ok(())
        }
    }

    // 1. Use a channel to synchronize the test with the mock output
    let (output_alert_tx, mut output_alert_rx) = mpsc::channel(1);
    let mock_output = Arc::new(helpers::mock_output::ChannelOutput::new(output_alert_tx));

    // 2. Create a broadcast channel for notifications
    let (notification_alert_tx, _) = broadcast::channel(100);
    let fake_slack_client = Arc::new(FakeSlackClient::new());

    // 2. Define a simple rule and the domain that will match it
    let matching_domain = "test.com";
    let rules = r#"
rules:
  - name: Test Rule
    domain_regex: "test.com"
    enrichment_level: "none"
"#;

    // 3. Build the test application
    let test_app = TestAppBuilder::new()
        .with_outputs(vec![mock_output])
        .with_rules(rules)
        .await
        .with_test_domains_channel()
        .with_metrics()
        .with_skipped_health_check()
        .with_alert_tx(notification_alert_tx) // Pass the notification channel sender
        .with_slack_notification(fake_slack_client.clone(), "test-channel") // Enable Slack notifications
        .start()
        .await
        .unwrap();

    // 4. Send the matching domain to trigger an alert
    test_app.send_domain(matching_domain).await.unwrap();

    // 5. Wait for the alert to be processed by the mock output
    let received_alert = tokio::time::timeout(Duration::from_secs(5), output_alert_rx.recv())
        .await
        .expect("Test timed out waiting for alert")
        .expect("Failed to receive alert from channel");

    assert_eq!(received_alert.domain, matching_domain);

    // 6. Fetch metrics from the server
    let client = reqwest::Client::new();
    let response_text = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // 7. Assert that the histogram metric for queue time has been recorded
    assert!(
        response_text.contains("alert_queue_time_seconds_sum"),
        "Metrics output should contain 'alert_queue_time_seconds_sum'. Got:\n{}",
        response_text
    );

    // 8. Shutdown the app
    test_app.shutdown(Duration::from_secs(1)).await.unwrap();
}