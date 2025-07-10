//! A mock Slack client for testing notification integration.

use async_trait::async_trait;
use certwatch::core::AggregatedAlert;
use certwatch::notification::slack::SlackClientTrait;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct MockSlackClient {
    pub sent_batches: Arc<Mutex<Vec<Vec<AggregatedAlert>>>>,
}

impl MockSlackClient {
    // The `new` and `get_sent_batches` functions were removed as they are no longer used.
    // The struct is now instantiated directly in the tests.
}

#[async_trait]
impl SlackClientTrait for MockSlackClient {
    async fn send_batch(&self, alerts: &[AggregatedAlert]) -> anyhow::Result<()> {
        let mut batches = self.sent_batches.lock().unwrap();
        batches.push(alerts.to_vec());
        Ok(())
    }
}