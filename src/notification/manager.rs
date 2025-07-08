//! The notification manager is a stateful actor responsible for batching
//! alerts and sending them to a notification service like Slack.

use crate::config::SlackConfig;
use crate::core::Alert;
use crate::notification::slack::SlackClientTrait;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// The `NotificationManager` actor.
pub struct NotificationManager<S: SlackClientTrait> {
    config: SlackConfig,
    alert_rx: broadcast::Receiver<Alert>,
    slack_client: Arc<S>,
}

impl<S: SlackClientTrait> NotificationManager<S> {
    /// Creates a new `NotificationManager`.
    pub fn new(
        config: SlackConfig,
        alert_rx: broadcast::Receiver<Alert>,
        slack_client: Arc<S>,
    ) -> Self {
        Self {
            config,
            alert_rx,
            slack_client,
        }
    }

    /// Runs the notification manager's main loop.
    pub async fn run(mut self) {
        let mut batch = Vec::with_capacity(self.config.batch_size);
        let mut timer = interval(Duration::from_secs(self.config.batch_timeout_seconds));

        loop {
            tokio::select! {
                biased;
                _ = timer.tick() => {
                    if !batch.is_empty() {
                        debug!("Batch timer expired, sending {} alerts", batch.len());
                        self.send_batch(&mut batch).await;
                    }
                }
                result = self.alert_rx.recv() => {
                    match result {
                        Ok(alert) => {
                            batch.push(alert);
                            if batch.len() >= self.config.batch_size {
                                debug!("Batch size limit reached, sending {} alerts", batch.len());
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
                                debug!("Sending final batch of {} alerts.", batch.len());
                                self.send_batch(&mut batch).await;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Sends the current batch of alerts and clears the batch.
    async fn send_batch(&self, batch: &mut Vec<Alert>) {
        if let Err(e) = self.slack_client.send_batch(batch).await {
            error!("Failed to send Slack notification batch: {}", e);
            // TODO: Add metrics for failed sends
        }
        batch.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SlackConfig;
    use crate::core::Alert;
    use crate::notification::slack::SlackClientTrait;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use tokio::sync::broadcast;
    use tokio::time::{advance, pause, Duration};

    // A fake Slack client for testing the manager's batching logic.
    #[derive(Clone)]
    struct FakeSlackClient {
        sent_batches: Arc<Mutex<Vec<Vec<Alert>>>>,
    }

    impl FakeSlackClient {
        fn new() -> Self {
            Self {
                sent_batches: Arc::new(Mutex::new(Vec::new())),
            }
        }

        // A test helper to get the batches that were "sent".
        fn get_sent_batches(&self) -> Vec<Vec<Alert>> {
            self.sent_batches.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SlackClientTrait for FakeSlackClient {
        // A fake implementation of send_batch that just records the batch.
        async fn send_batch(&self, alerts: &[Alert]) -> anyhow::Result<()> {
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
            enabled: true,
            webhook_url: "fake".to_string(),
            batch_size: 2,
            batch_timeout_seconds: 10,
        };

        let manager = NotificationManager::new(config, alert_rx, fake_client.clone());
        let manager_handle = tokio::spawn(manager.run());

        // Act
        alert_tx.send(Alert::default()).unwrap();
        alert_tx.send(Alert::default()).unwrap();

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
            enabled: true,
            webhook_url: "fake".to_string(),
            batch_size: 10,
            batch_timeout_seconds: 5,
        };

        let manager = NotificationManager::new(config, alert_rx, fake_client.clone());
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

        // Cleanup
        manager_handle.abort();
    }
}