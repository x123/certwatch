//! The notification manager is a stateful actor responsible for batching
//! alerts and sending them to a notification service like Slack.

use crate::config::{DeduplicationConfig, SlackConfig};
use crate::core::{Alert, AggregatedAlert};
use crate::notification::slack::SlackClientTrait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// The `NotificationManager` actor.
pub struct NotificationManager<S: SlackClientTrait + ?Sized> {
    config: SlackConfig,
    alert_rx: broadcast::Receiver<Alert>,
    slack_client: Arc<S>,
}

impl<S: SlackClientTrait + ?Sized> NotificationManager<S> {
    /// Creates a new `NotificationManager`.
    pub fn new(
        slack_config: SlackConfig,
        _deduplication_config: &DeduplicationConfig,
        alert_rx: broadcast::Receiver<Alert>,
        slack_client: Arc<S>,
    ) -> Self {
        Self {
            config: slack_config,
            alert_rx,
            slack_client,
        }
    }

    /// Runs the notification manager's main loop.
    pub async fn run(mut self) {
        info!("NotificationManager started.");
        let batch_size = self.config.batch_size.unwrap_or(50);
        let batch_timeout = self.config.batch_timeout_seconds.unwrap_or(300);
        debug!(
            batch_size = batch_size,
            batch_timeout = batch_timeout,
            "NotificationManager configured"
        );

        let mut batch: HashMap<String, AggregatedAlert> = HashMap::with_capacity(batch_size);
        let mut timer = interval(Duration::from_secs(batch_timeout));

        loop {
            tokio::select! {
                biased;
                _ = timer.tick() => {
                    if !batch.is_empty() {
                        info!("Batch timer expired, sending {} aggregated alerts to Slack.", batch.len());
                        self.send_batch(&mut batch).await;
                    }
                }
                result = self.alert_rx.recv() => {
                    match result {
                        Ok(alert) => {
                            if let Some(start_time) = alert.notification_queue_start_time {
                                let duration = start_time.elapsed().as_secs_f64();
                                metrics::histogram!("alert_queue_time_seconds").record(duration);
                            }
                            debug!(domain = %alert.domain, "Received alert in NotificationManager.");
                            let base_domain = psl::domain_str(&alert.domain).unwrap_or(&alert.domain).to_string();
                            
                            if let Some(existing_alert) = batch.get_mut(&base_domain) {
                                existing_alert.deduplicated_count += 1;
                                debug!(domain = %alert.domain, "Aggregated subdomain.");
                            } else {
                                batch.insert(base_domain, AggregatedAlert { alert, deduplicated_count: 0 });
                            }

                            if batch.len() >= batch_size {
                                info!("Batch size limit reached, sending {} aggregated alerts to Slack.", batch.len());
                                self.send_batch(&mut batch).await;
                                timer.reset();
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            error!("NotificationManager lagged, dropping {} alerts.", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Alert channel closed. Shutting down NotificationManager.");
                            if !batch.is_empty() {
                                info!("Sending final batch of {} aggregated alerts before shutdown.", batch.len());
                                self.send_batch(&mut batch).await;
                            }
                            break;
                        }
                    }
                }
            }
        }
        info!("NotificationManager finished.");
    }

    /// Sends the current batch of alerts and clears the batch.
    async fn send_batch(&self, batch: &mut HashMap<String, AggregatedAlert>) {
        if batch.is_empty() {
            return;
        }
        let alerts: Vec<AggregatedAlert> = batch.values().cloned().collect();
        if let Err(e) = self.slack_client.send_batch(&alerts).await {
            error!("Failed to send Slack notification batch: {}", e);
            // TODO: Add metrics for failed sends
        }
        batch.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeduplicationConfig, SlackConfig};
    use crate::core::Alert;
    use crate::notification::slack::SlackClientTrait;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use tokio::sync::broadcast;
    use tokio::time::{advance, pause, Duration};

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

    #[tokio::test]
    async fn test_notification_manager_sends_on_batch_size() {
        // Arrange
        let (alert_tx, alert_rx) = broadcast::channel(100);
        let fake_client = Arc::new(FakeSlackClient::new());
        let config = SlackConfig {
            enabled: Some(true),
            webhook_url: Some("fake".to_string()),
            batch_size: Some(2),
            batch_timeout_seconds: Some(10),
        };

        let deduplication_config = DeduplicationConfig::default();
        let manager = NotificationManager::new(config, &deduplication_config, alert_rx, fake_client.clone());
        let manager_handle = tokio::spawn(manager.run());

        // Act
        let mut alert1 = Alert::default();
        alert1.domain = "example.com".to_string();
        let mut alert2 = Alert::default();
        alert2.domain = "sub.example.com".to_string();
        let mut alert3 = Alert::default();
        alert3.domain = "another.com".to_string();

        alert_tx.send(alert1).unwrap();
        alert_tx.send(alert2).unwrap();
        alert_tx.send(alert3).unwrap();

        // Allow some time for the manager to process the alerts
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Assert
        let batches = fake_client.get_sent_batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 2);

        // Cleanup
        manager_handle.abort();
    }

    #[tokio::test]
    async fn test_notification_manager_sends_on_timeout() {
        // Arrange
        pause(); // Pause time
        let (alert_tx, alert_rx) = broadcast::channel(100);
        let fake_client = Arc::new(FakeSlackClient::new());
        let config = SlackConfig {
            enabled: Some(true),
            webhook_url: Some("fake".to_string()),
            batch_size: Some(10),
            batch_timeout_seconds: Some(5),
        };

        let deduplication_config = DeduplicationConfig::default();
        let manager = NotificationManager::new(config, &deduplication_config, alert_rx, fake_client.clone());
        let manager_handle = tokio::spawn(manager.run());

        // Act
        alert_tx.send(Alert::default()).unwrap();

        // Advance time past the timeout
        advance(Duration::from_secs(6)).await;
        // Yield to allow the timer to be processed
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Assert
        let batches = fake_client.get_sent_batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(batches[0][0].deduplicated_count, 0);

        // Cleanup
        manager_handle.abort();
    }
}