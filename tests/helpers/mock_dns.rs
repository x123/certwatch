#![allow(dead_code)]
//! A mock DNS resolver for testing purposes.

use async_trait::async_trait;
use certwatch::{
    core::{DnsInfo, DnsResolver},
    dns::DnsError,
    internal_metrics::Metrics,
};
use std::{
    collections::{HashMap, HashSet},
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
    panic_on_resolve: bool,
    allowed_domains: Arc<Mutex<HashSet<String>>>,
}

impl MockDnsResolver {
    pub fn new() -> Self {
        Self::new_with_metrics(Arc::new(Metrics::disabled()))
    }

    pub fn new_with_metrics(metrics: Arc<Metrics>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            resolve_counts: Arc::new(Mutex::new(HashMap::new())),
            metrics,
            resolve_attempts_tx: Arc::new(Mutex::new(None)),
            delay: None,
            completion_tx: Arc::new(Mutex::new(None)),
            panic_on_resolve: false,
            allowed_domains: Arc::new(Mutex::new(HashSet::new())),
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
    pub fn with_panic_on_resolve(mut self) -> Self {
        self.panic_on_resolve = true;
        self
    }

    pub fn with_allowed_domain(self, domain: &str) -> Self {
        self.allowed_domains.lock().unwrap().insert(domain.to_string());
        self
    }
}

#[async_trait]
impl DnsResolver for MockDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        if self.panic_on_resolve {
            if self.allowed_domains.lock().unwrap().contains(domain) {
                // If in panic mode but the domain is allowed, return a success.
                // This simulates a successful resolution without actually panicking.
                self.metrics.increment_dns_query("success");
                return Ok(DnsInfo::default());
            } else {
                // If in panic mode and the domain is not allowed, panic.
                panic!("MockDnsResolver: resolve called for '{}' while in panic_on_resolve mode and domain is not allowed.", domain);
            }
        }

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