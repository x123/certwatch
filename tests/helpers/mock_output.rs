#![allow(dead_code)]
use async_trait::async_trait;
use certwatch::core::{Alert, Output};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::{mpsc, oneshot, Notify};
use tracing::debug;

/// A mock Output that counts the number of alerts it has received.
#[derive(Debug)]
pub struct CountingOutput {
    pub count: Arc<AtomicUsize>,
    pub notifier: Arc<Notify>,
    completion_tx: Mutex<Option<oneshot::Sender<()>>>,
    target_count: usize,
}

impl CountingOutput {
    pub fn new(target_count: usize) -> Self {
        Self {
            count: Arc::new(AtomicUsize::new(0)),
            notifier: Arc::new(Notify::new()),
            completion_tx: Mutex::new(None),
            target_count,
        }
    }

    pub fn with_completion_signal(self, tx: oneshot::Sender<()>) -> Self {
        *self.completion_tx.lock().unwrap() = Some(tx);
        self
    }

    pub async fn wait_for_count(&self) {
        let wait_future = async {
            while self.count.load(Ordering::SeqCst) < self.target_count {
                self.notifier.notified().await;
            }
        };
        wait_future.await;
    }
}

#[async_trait]
impl Output for CountingOutput {
    fn name(&self) -> &str {
        "counting_mock"
    }

    async fn send_alert(&self, _alert: &Alert) -> anyhow::Result<()> {
        let current_count = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        debug!(
            "CountingOutput: Received alert for '{}'. Count is now {}.",
            _alert.domain, current_count
        );
        self.notifier.notify_one();

        if current_count == self.target_count {
            debug!(
                "CountingOutput: Reached target count of {}. Sending completion signal.",
                self.target_count
            );
            if let Some(tx) = self.completion_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
        Ok(())
    }
}

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