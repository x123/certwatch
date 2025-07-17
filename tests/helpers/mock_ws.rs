#![allow(dead_code)]
//! Mock WebSocketConnection for testing the CertStreamClient
use async_trait::async_trait;
use certwatch::network::WebSocketConnection;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::{oneshot, Barrier};
use tokio_tungstenite::tungstenite::{Error, Message};

#[derive(Clone)]
pub struct MockWebSocket {
    messages: Arc<Mutex<VecDeque<Result<Message, Error>>>>,
    is_closed: Arc<AtomicBool>,
    // Signals to the test that the app has called `read_message` at least once.
    ready_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    // Synchronizes the test and the mock to prevent race conditions.
    barrier: Option<Arc<Barrier>>,
}

impl MockWebSocket {
    pub fn new(barrier: Arc<Barrier>) -> (Self, oneshot::Receiver<()>) {
        let (ready_tx, ready_rx) = oneshot::channel();
        let mock_ws = Self {
            messages: Arc::new(Mutex::new(VecDeque::new())),
            is_closed: Arc::new(AtomicBool::new(false)),
            ready_tx: Arc::new(Mutex::new(Some(ready_tx))),
            barrier: Some(barrier),
        };
        (mock_ws, ready_rx)
    }

    pub fn new_silent() -> Self {
        Self {
            messages: Arc::new(Mutex::new(VecDeque::new())),
            is_closed: Arc::new(AtomicBool::new(false)),
            ready_tx: Arc::new(Mutex::new(None)),
            barrier: None,
        }
    }

    pub fn add_domain_message(&self, domain: &str) {
        let json = format!(r#"{{"data": {{"leaf_cert": {{"all_domains": ["{}"]}}}}}}"#, domain);
        self.messages
            .lock()
            .unwrap()
            .push_back(Ok(Message::Text(json.into())));
    }

    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl WebSocketConnection for MockWebSocket {
    async fn read_message(&mut self) -> Option<Result<Message, Error>> {
        // On the first call, signal that the application is ready.
        if let Some(tx) = self.ready_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }

        // Wait for the test to signal it's ready for us to proceed.
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        } else {
            // If there's no barrier, this is a silent mock that should close immediately.
            return None;
        }

        if self.is_closed.load(Ordering::SeqCst) {
            return None;
        }

        self.messages.lock().unwrap().pop_front()
    }
}