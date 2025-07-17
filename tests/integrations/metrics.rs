#[path = "../helpers/mod.rs"]
mod helpers;

use certwatch::core::{Alert, DnsInfo, Output, AggregatedAlert};
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver};
use std::{net::Ipv4Addr, sync::{Arc, Mutex}, time::Duration};
use tokio::sync::{mpsc, oneshot, broadcast};
use async_trait::async_trait;
use certwatch::notification::slack::SlackClientTrait;

/// A mock Output that sends the received alert over a channel.
#[derive(Clone, Debug)]
pub struct ChannelOutput {
    pub alert_tx: mpsc::Sender<Alert>,
}

impl ChannelOutput {
    pub fn new(alert_tx: mpsc::Sender<Alert>) -> Self {
        Self { alert_tx }
    }
}

#[async_trait]
impl Output for ChannelOutput {
    fn name(&self) -> &str {
        "channel_mock"
    }

    async fn send_alert(&self, alert: &Alert) -> anyhow::Result<()> {
        self.alert_tx.send(alert.clone()).await?;
        Ok(())
    }
}

#[tokio::test]
async fn test_dns_worker_scheduling_delay_metric() {
    let (completion_tx, completion_rx) = oneshot::channel();

    // 1. Configure the TestAppBuilder with a low number of DNS workers
    let mock_dns = MockDnsResolver::new(Arc::new(certwatch::internal_metrics::Metrics::new_for_test()))
        .with_delay(Duration::from_millis(100))
        .with_completion_signal(completion_tx);

    mock_dns.add_response(
        "one.com",
        Ok(DnsInfo {
            a_records: vec![Ipv4Addr::new(1, 1, 1, 1).into()],
            ..Default::default()
        }),
    );

    let test_app = TestAppBuilder::new()
        .with_config_modifier(|config| {
            config.performance.dns_worker_concurrency = 1;
        })
        .with_dns_resolver(Arc::new(mock_dns))
        .with_test_domains_channel()
        .with_metrics()
        .with_skipped_health_check()
        .start()
        .await
        .unwrap();

    // 2. Inject domains into the app
    test_app.send_domain("one.com").await.unwrap();

    // 3. Wait for the DNS resolution to complete
    completion_rx
        .await
        .expect("Failed to receive completion signal");

    // 4. Fetch metrics and assert
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let count_line = response
        .lines()
        .find(|line| line.starts_with("dns_worker_scheduling_delay_seconds_count"))
        .unwrap();
    let count: u64 = count_line.split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(count, 1, "The count for the metric should be 1");

    let sum_line = response
        .lines()
        .find(|line| line.starts_with("dns_worker_scheduling_delay_seconds_sum"))
        .unwrap();
    let sum: f64 = sum_line.split_whitespace().last().unwrap().parse().unwrap();
    assert!(sum > 0.0, "The sum should be greater than 0");

    test_app.shutdown(Duration::from_secs(1)).await.unwrap();
}

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

        // A test helper to get the batches that were "sent".
        fn get_sent_batches(&self) -> Vec<Vec<AggregatedAlert>> {
            self.sent_batches.lock().unwrap().clone()
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
    let mock_output = Arc::new(ChannelOutput::new(output_alert_tx));

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