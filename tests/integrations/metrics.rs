#[path = "../helpers/mod.rs"]
mod helpers;

use certwatch::core::AggregatedAlert;
use helpers::app::TestAppBuilder;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, oneshot};
use async_trait::async_trait;
use certwatch::notification::slack::SlackClientTrait;

#[tokio::test]
#[ignore]
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

    // 1. Use a oneshot channel to synchronize the test with the mock output
    let (completion_tx, completion_rx) = oneshot::channel();
    let mock_output = Arc::new(helpers::mock_output::CountingOutput::new(1).with_completion_signal(completion_tx));

    // 2. Create a broadcast channel for notifications
    let (notification_alert_tx, _) = broadcast::channel(100);
    let fake_slack_client = Arc::new(FakeSlackClient::new());

    // 3. Define a simple rule and the domain that will match it
    let matching_domain = "test.com";
    let rules = r#"
rules:
  - name: Test Rule
    domain_regex: "test.com"
    enrichment_level: "none"
"#;

    // 4. Build the test application
    let test_app_builder = TestAppBuilder::new()
        .with_outputs(vec![mock_output.clone()])
        .with_rules(rules)
        .with_test_domains_channel()
        .with_metrics()
        .with_skipped_health_check()
        .with_alert_tx(notification_alert_tx) // Pass the notification channel sender
        .with_slack_notification(fake_slack_client.clone(), "test-channel"); // Enable Slack notifications

    let test_app = test_app_builder.start().await.unwrap();
    test_app.wait_for_startup().await.unwrap();

    // 5. Send the matching domain to trigger an alert
    test_app.send_domain(matching_domain).await.unwrap();

    // 6. Wait for the alert to be processed by the mock output
    completion_rx
        .await
        .expect("Failed to receive completion signal");

    assert_eq!(mock_output.count.load(std::sync::atomic::Ordering::SeqCst), 1);

    // 6. Fetch metrics from the server
    let client = reqwest::Client::new();
    let response_text = client
        .get(format!("http://{}/metrics", test_app.metrics_addr().unwrap()))
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
    test_app.shutdown().await.unwrap();
}