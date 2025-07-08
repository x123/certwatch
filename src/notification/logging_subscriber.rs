//! A simple subscriber that logs received alerts to the console.
//!
//! This serves as a basic implementation to validate the notification pipeline
//! and can be used for debugging purposes.

use crate::core::Alert;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::broadcast;
use tracing::{info, instrument, warn};

/// Spawns a task that listens for alerts on a broadcast channel and logs them.
#[instrument(skip_all)]
pub fn spawn(mut alert_rx: broadcast::Receiver<Alert>, confirmation_flag: Option<Arc<AtomicBool>>) {
    tokio::spawn(async move {
        println!("[DEBUG] LoggingSubscriber spawned.");
        info!("LoggingSubscriber started.");
        loop {
            match alert_rx.recv().await {
                Ok(alert) => {
                    println!("[DEBUG] LoggingSubscriber received alert for domain: {}", alert.domain);
                    info!(?alert, "Received alert via notification channel");
                    if let Some(flag) = &confirmation_flag {
                        println!("[DEBUG] Setting confirmation flag for domain: {}", alert.domain);
                        flag.store(true, Ordering::SeqCst);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("LoggingSubscriber lagged behind and missed {} alerts.", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Alert channel closed. LoggingSubscriber shutting down.");
                    break;
                }
            }
        }
    });
}