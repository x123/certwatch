#![allow(dead_code)]
use async_trait::async_trait;
use certwatch::core::{Alert, Output};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

/// A mock Output that counts the number of alerts it has received.
#[derive(Clone, Debug)]
pub struct CountingOutput {
    pub count: Arc<AtomicUsize>,
    pub notifier: Arc<Notify>,
}

impl CountingOutput {
    pub fn new() -> Self {
        Self {
            count: Arc::new(AtomicUsize::new(0)),
            notifier: Arc::new(Notify::new()),
        }
    }
}

#[async_trait]
impl Output for CountingOutput {
    fn name(&self) -> &str {
        "counting_mock"
    }

    async fn send_alert(&self, _alert: &Alert) -> anyhow::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.notifier.notify_one();
        Ok(())
    }
}