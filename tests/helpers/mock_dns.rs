#![allow(dead_code)]
//! A mock DNS resolver for testing purposes.

use async_trait::async_trait;
use certwatch::{
    core::{DnsInfo, DnsResolver},
    dns::DnsError,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Default)]
pub struct MockDnsResolver {
    responses: Arc<Mutex<HashMap<String, Result<DnsInfo, DnsError>>>>,
    resolve_counts: Arc<Mutex<HashMap<String, u32>>>,
}

impl MockDnsResolver {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            resolve_counts: Arc::new(Mutex::new(HashMap::new())),
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

        if let Some(response) = self.responses.lock().unwrap().get(domain) {
            response.clone()
        } else {
            // Default behavior for unconfigured domains: return an error
            // to prevent tests from accidentally passing due to unexpected success.
            Err(DnsError::Resolution(format!(
                "MockDnsResolver: No response configured for domain '{}'",
                domain
            )))
        }
    }
}