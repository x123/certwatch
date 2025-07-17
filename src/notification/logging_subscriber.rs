use crate::{core::Alert, task_manager::TaskManager};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Spawns a new task that subscribes to the alert broadcast channel and logs
/// every alert it receives.
pub fn spawn(
    mut alert_rx: broadcast::Receiver<Alert>,
    task_manager: &TaskManager,
    completion_tx: Option<tokio::sync::oneshot::Sender<()>>,
) {
    let mut shutdown_rx = task_manager.get_shutdown_rx();
    task_manager.spawn("LoggingSubscriber", async move {
        info!("Logging subscriber started.");
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.changed() => {
                    info!("Logging subscriber received shutdown signal.");
                    break;
                }
                result = alert_rx.recv() => {
                    match result {
                        Ok(alert) => {
                            info!(domain = %alert.domain, source = %alert.source_tag.join(","), "New alert received");
                            if let Some(tx) = completion_tx {
                                let _ = tx.send(());
                                return; // Test is done, exit the task
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Logging subscriber is lagging behind. Skipped {} messages.", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Alert channel closed. Logging subscriber shutting down.");
                            break;
                        }
                    }
                }
            }
        }
        info!("Logging subscriber finished.");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DnsInfo;
    use tokio::sync::{broadcast, oneshot, watch};

    #[tokio::test]
    async fn test_logging_subscriber_receives_and_logs_alert() {
        // Arrange
        let (alert_tx, alert_rx) = broadcast::channel(10);
        let (completion_tx, completion_rx) = oneshot::channel();

        // Spawn the subscriber with the completion channel.
        let (_, shutdown_rx) = watch::channel(false);
        let task_manager = TaskManager::new(shutdown_rx);
        spawn(alert_rx, &task_manager, Some(completion_tx));

        let alert = Alert {
            timestamp: "2025-07-08T18:00:00Z".to_string(),
            domain: "test.com".to_string(),
            source_tag: vec!["test-source".to_string()],
            resolved_after_nxdomain: false,
            dns: DnsInfo::default(),
            enrichment: vec![],
            processing_start_time: None,
            notification_queue_start_time: None,
        };

        // Act
        alert_tx.send(alert).unwrap();

        // Assert
        // Wait for the subscriber to signal that it has processed the alert.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), completion_rx)
            .await
            .expect("Test timed out waiting for subscriber to complete");
    }
}