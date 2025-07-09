use crate::{
    core::DnsInfo,
    dns::{DnsError, DnsResolver},
};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Fake DNS resolver for testing
pub struct FakeDnsResolver {
    // A queue of responses for a given domain. The front of the queue is the next response.
    responses: Arc<Mutex<HashMap<String, VecDeque<Result<DnsInfo, String>>>>>,
    call_count: Arc<Mutex<HashMap<String, u32>>>,
}

impl FakeDnsResolver {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            call_count: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a successful response to the queue for a domain
    pub fn add_success_response(&self, domain: &str, dns_info: DnsInfo) {
        let mut responses = self.responses.lock().unwrap();
        responses
            .entry(domain.to_string())
            .or_default()
            .push_back(Ok(dns_info));
    }

    /// Add an error response to the queue for a domain
    pub fn add_error_response(&self, domain: &str, error: &str) {
        let mut responses = self.responses.lock().unwrap();
        responses
            .entry(domain.to_string())
            .or_default()
            .push_back(Err(error.to_string()));
    }

    /// Get the number of times a domain was queried
    pub fn get_call_count(&self, domain: &str) -> u32 {
        let call_count = self.call_count.lock().unwrap();
        call_count.get(domain).copied().unwrap_or(0)
    }
}

#[async_trait]
impl DnsResolver for FakeDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        // Increment call count
        {
            let mut call_count = self.call_count.lock().unwrap();
            *call_count.entry(domain.to_string()).or_insert(0) += 1;
        }

        // Return the next configured response
        let mut responses = self.responses.lock().unwrap();
        if let Some(queue) = responses.get_mut(domain) {
            if let Some(response) = queue.pop_front() {
                return match response {
                    Ok(dns_info) => Ok(dns_info),
                    Err(error) => Err(DnsError::Resolution(error)),
                };
            }
        }
        Err(DnsError::Resolution(format!(
            "No more responses configured for {}",
            domain
        )))
    }
}