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
};

#[derive(Debug, Clone)]
pub struct MockDnsResolver {
    responses: Arc<Mutex<HashMap<String, Result<DnsInfo, DnsError>>>>,
    resolve_counts: Arc<Mutex<HashMap<String, u32>>>,
    metrics: Arc<Metrics>,
}

impl MockDnsResolver {
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            resolve_counts: Arc::new(Mutex::new(HashMap::new())),
            metrics,
        }
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

        response
    }
}