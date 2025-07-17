use crate::core::Alert;
use tokio::sync::{broadcast, watch};
use tracing::{info, warn};

/// Spawns a new task that subscribes to the alert broadcast channel and logs
/// every alert it receives.
pub fn spawn(
    mut alert_rx: broadcast::Receiver<Alert>,
    mut shutdown_rx: Option<watch::Receiver<()>>,
    completion_tx: Option<tokio::sync::oneshot::Sender<()>>,
) {
    tokio::spawn(async move {
        info!("Logging subscriber started.");
        loop {
            // Check for shutdown signal if one is provided
            if let Some(rx) = shutdown_rx.as_mut() {
                tokio::select! {
                    biased;
                    _ = rx.changed() => {
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
            } else {
                // If no shutdown receiver, just listen for alerts
                match alert_rx.recv().await {
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
        info!("Logging subscriber finished.");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DnsInfo;
    use tokio::sync::{broadcast, oneshot};

    #[tokio::test]
    async fn test_logging_subscriber_receives_and_logs_alert() {
        // Arrange
        let (alert_tx, alert_rx) = broadcast::channel(10);
        let (completion_tx, completion_rx) = oneshot::channel();

        // Spawn the subscriber with the completion channel.
        spawn(alert_rx, None, Some(completion_tx));

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