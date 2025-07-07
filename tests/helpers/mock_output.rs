//! A mock implementation of the `Output` trait for testing purposes.

use anyhow::Result;
use async_trait::async_trait;
use certwatch::core::{Alert, Output};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// A mock output that can be configured to fail and records the alerts it receives.
#[derive(Debug, Clone)]
pub struct FailableMockOutput {
    pub alerts: Arc<Mutex<Vec<Alert>>>,
    pub fail_on_send: Arc<AtomicBool>,
}

impl FailableMockOutput {
    /// Creates a new `FailableMockOutput`.
    pub fn new() -> Self {
        Self {
            alerts: Arc::new(Mutex::new(Vec::new())),
            fail_on_send: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Configures the mock to fail on the next `send_alert` call.
    pub fn set_fail_on_send(&self, fail: bool) {
        self.fail_on_send.store(fail, Ordering::SeqCst);
    }
}

impl Default for FailableMockOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Output for FailableMockOutput {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        if self.fail_on_send.load(Ordering::SeqCst) {
            anyhow::bail!("Mock output configured to fail");
        } else {
            self.alerts.lock().unwrap().push(alert.clone());
            Ok(())
        }
    }
}