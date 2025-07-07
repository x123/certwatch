//! Mock WebSocketConnection for testing the CertStreamClient
use async_trait::async_trait;
use certwatch::network::WebSocketConnection;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::{Error, Message};

#[derive(Clone)]
pub struct MockWebSocket {
    messages: Arc<Mutex<VecDeque<Result<Message, Error>>>>,
    is_closed: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl MockWebSocket {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(VecDeque::new())),
            is_closed: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn add_message(&self, msg: Message) {
        self.messages.lock().unwrap().push_back(Ok(msg));
        self.notify.notify_one();
    }

    pub fn add_domain_message(&self, domain: &str) {
        let json = format!(r#"{{"data": ["{}"]}}"#, domain);
        self.add_message(Message::Text(json.into()));
    }

    pub fn add_error(&self, error: Error) {
        self.messages.lock().unwrap().push_back(Err(error));
        self.notify.notify_one();
    }

    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

#[async_trait]
impl WebSocketConnection for MockWebSocket {
    async fn read_message(&mut self) -> Option<Result<Message, Error>> {
        loop {
            if self.is_closed.load(Ordering::SeqCst) {
                return None;
            }

            if let Some(msg) = self.messages.lock().unwrap().pop_front() {
                return Some(msg);
            }

            self.notify.notified().await;
        }
    }
}