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
}

impl MockDnsResolver {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_response(&self, domain: &str, response: Result<DnsInfo, DnsError>) {
        self.responses
            .lock()
            .unwrap()
            .insert(domain.to_string(), response);
    }
}

#[async_trait]
impl DnsResolver for MockDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
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