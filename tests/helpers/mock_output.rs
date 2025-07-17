#![allow(dead_code)]
use async_trait::async_trait;
use certwatch::core::{Alert, Output};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::{mpsc, Notify};

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

    pub async fn wait_for_count(&self, target_count: usize, timeout_duration: std::time::Duration) {
        let wait_future = async {
            while self.count.load(Ordering::SeqCst) < target_count {
                self.notifier.notified().await;
            }
        };

        tokio::time::timeout(timeout_duration, wait_future)
            .await
            .expect("Timed out waiting for alerts");
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