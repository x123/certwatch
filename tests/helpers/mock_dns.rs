#![allow(dead_code)]
//! A mock DNS resolver for testing purposes.

use async_trait::async_trait;
use certwatch::{
    core::{DnsInfo, DnsResolver},
    dns::DnsError,
    internal_metrics::Metrics,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct MockDnsResolver {
    responses: Arc<Mutex<HashMap<String, Result<DnsInfo, DnsError>>>>,
    resolve_counts: Arc<Mutex<HashMap<String, u32>>>,
    metrics: Arc<Metrics>,
    /// A channel to signal when a resolve attempt has been made.
    resolve_attempts_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    delay: Option<Duration>,
    completion_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl MockDnsResolver {
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            resolve_counts: Arc::new(Mutex::new(HashMap::new())),
            metrics,
            resolve_attempts_tx: Arc::new(Mutex::new(None)),
            delay: None,
            completion_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    pub fn with_completion_signal(self, tx: tokio::sync::oneshot::Sender<()>) -> Self {
        *self.completion_tx.lock().unwrap() = Some(tx);
        self
    }

    /// Returns a receiver that will get a message every time `resolve` is called.
    pub fn get_resolve_attempts_rx(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel(100);
        *self.resolve_attempts_tx.lock().unwrap() = Some(tx);
        rx
    }

    pub fn add_response(&self, domain: &str, response: Result<DnsInfo, DnsError>) {
        self.responses
            .lock()
            .unwrap()
            .insert(domain.to_string(), response);
    }

    pub fn get_resolve_count(&self, domain: &str) -> u32 {
        self.resolve_counts
            .lock()
            .unwrap()
            .get(domain)
            .cloned()
            .unwrap_or(0)
    }

    pub async fn add_failure(&self, domain: &str) {
        self.add_response(domain, Err(DnsError::Resolution("Simulated DNS failure".to_string())));
    }
}

#[async_trait]
impl DnsResolver for MockDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        // Signal that a resolve attempt was made.
        if let Some(tx) = self.resolve_attempts_tx.lock().unwrap().as_ref() {
            let _ = tx.try_send(domain.to_string());
        }

        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }

        // Increment the resolve count for this domain.
        let mut counts = self.resolve_counts.lock().unwrap();
        *counts.entry(domain.to_string()).or_insert(0) += 1;

        let response = self
            .responses
            .lock()
            .unwrap()
            .get(domain)
            .cloned()
            .unwrap_or_else(|| {
                Err(DnsError::Resolution(format!(
                    "MockDnsResolver: No response configured for domain '{}'",
                    domain
                )))
            });

        // Simulate metric recording
        match &response {
            Ok(_) => self.metrics.increment_dns_query("success"),
            Err(_) => self.metrics.increment_dns_query("failure"),
        }

        if let Some(tx) = self.completion_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }

        response
    }
}