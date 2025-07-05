//! DNS resolution service with dual-curve retry logic
//!
//! This module implements DNS resolution with separate retry strategies
//! for standard failures and NXDOMAIN responses.

use crate::core::{DnsInfo, DnsResolver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

/// Configuration for DNS retry policies
#[derive(Debug, Clone)]
pub struct DnsRetryConfig {
    /// Number of retries for standard failures (timeouts, server errors)
    pub standard_retries: u32,
    /// Initial backoff delay for standard failures in milliseconds
    pub standard_initial_backoff_ms: u64,
    /// Number of retries for NXDOMAIN responses
    pub nxdomain_retries: u32,
    /// Initial backoff delay for NXDOMAIN responses in milliseconds
    pub nxdomain_initial_backoff_ms: u64,
}

impl Default for DnsRetryConfig {
    fn default() -> Self {
        Self {
            standard_retries: 3,
            standard_initial_backoff_ms: 500,
            nxdomain_retries: 5,
            nxdomain_initial_backoff_ms: 10000,
        }
    }
}

/// DNS resolver implementation using trust-dns-resolver
pub struct TrustDnsResolver {
    resolver: TokioAsyncResolver,
}

impl TrustDnsResolver {
    /// Creates a new DNS resolver with default configuration
    pub fn new() -> Result<Self> {
        let resolver = TokioAsyncResolver::tokio(
            ResolverConfig::default(),
            ResolverOpts::default(),
        );
        Ok(Self { resolver })
    }

    /// Creates a new DNS resolver with custom configuration
    pub fn with_config(config: ResolverConfig, opts: ResolverOpts) -> Self {
        let resolver = TokioAsyncResolver::tokio(config, opts);
        Self { resolver }
    }
}

#[async_trait]
impl DnsResolver for TrustDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo> {
        let mut dns_info = DnsInfo::default();

        // Resolve A records (IPv4)
        match self.resolver.ipv4_lookup(domain).await {
            Ok(lookup) => {
                dns_info.a_records = lookup
                    .iter()
                    .map(|ip| IpAddr::V4(ip.0))
                    .collect();
            }
            Err(e) => {
                // Check if this is an NXDOMAIN error
                if e.to_string().contains("NXDOMAIN") {
                    return Err(anyhow!("NXDOMAIN: {}", domain));
                }
                // For other errors, we'll still try other record types
                log::warn!("Failed to resolve A records for {}: {}", domain, e);
            }
        }

        // Resolve AAAA records (IPv6)
        match self.resolver.ipv6_lookup(domain).await {
            Ok(lookup) => {
                dns_info.aaaa_records = lookup
                    .iter()
                    .map(|ip| IpAddr::V6(ip.0))
                    .collect();
            }
            Err(e) => {
                log::warn!("Failed to resolve AAAA records for {}: {}", domain, e);
            }
        }

        // Resolve NS records
        match self.resolver.ns_lookup(domain).await {
            Ok(lookup) => {
                dns_info.ns_records = lookup
                    .iter()
                    .map(|ns| ns.to_string())
                    .collect();
            }
            Err(e) => {
                log::warn!("Failed to resolve NS records for {}: {}", domain, e);
            }
        }

        // If we got no records at all and no NXDOMAIN, it's likely a resolution failure
        if dns_info.a_records.is_empty() 
            && dns_info.aaaa_records.is_empty() 
            && dns_info.ns_records.is_empty() {
            return Err(anyhow!("No DNS records found for {}", domain));
        }

        Ok(dns_info)
    }
}

/// Manages DNS resolution with dual-curve retry logic
pub struct DnsResolutionManager {
    resolver: Arc<dyn DnsResolver>,
    config: DnsRetryConfig,
    nxdomain_retry_tx: mpsc::UnboundedSender<(String, String, Instant)>, // (domain, source_tag, retry_time)
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager
    pub fn new(resolver: Arc<dyn DnsResolver>, config: DnsRetryConfig) -> Self {
        let (nxdomain_retry_tx, nxdomain_retry_rx) = mpsc::unbounded_channel();
        
        // Spawn the NXDOMAIN retry task
        let resolver_clone = Arc::clone(&resolver);
        let config_clone = config.clone();
        tokio::spawn(Self::nxdomain_retry_task(resolver_clone, config_clone, nxdomain_retry_rx));

        Self {
            resolver,
            config,
            nxdomain_retry_tx,
        }
    }

    /// Attempts to resolve a domain with standard retry logic
    pub async fn resolve_with_retry(&self, domain: &str, source_tag: &str) -> Result<DnsInfo> {
        let mut last_error = None;
        
        for attempt in 0..=self.config.standard_retries {
            match self.resolver.resolve(domain).await {
                Ok(dns_info) => return Ok(dns_info),
                Err(e) => {
                    last_error = Some(format!("{}", e));
                    
                    // Check if this is an NXDOMAIN error
                    if e.to_string().contains("NXDOMAIN") {
                        // Schedule for NXDOMAIN retry queue
                        let retry_time = Instant::now() + Duration::from_millis(self.config.nxdomain_initial_backoff_ms);
                        if let Err(send_err) = self.nxdomain_retry_tx.send((domain.to_string(), source_tag.to_string(), retry_time)) {
                            log::error!("Failed to schedule NXDOMAIN retry: {}", send_err);
                        }
                        
                        // Return the NXDOMAIN error immediately for immediate alert generation
                        return Err(e);
                    }
                    
                    // For standard errors, apply exponential backoff
                    if attempt < self.config.standard_retries {
                        let backoff_ms = self.config.standard_initial_backoff_ms * 2_u64.pow(attempt);
                        sleep(Duration::from_millis(backoff_ms)).await;
                    }
                }
            }
        }

        Err(anyhow!(last_error.unwrap_or_else(|| format!("DNS resolution failed after {} retries", self.config.standard_retries))))
    }

    /// Background task that handles NXDOMAIN retries
    async fn nxdomain_retry_task(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        mut rx: mpsc::UnboundedReceiver<(String, String, Instant)>,
    ) {
        let mut retry_queue: Vec<(String, String, Instant, u32)> = Vec::new(); // (domain, source_tag, next_retry, attempt)

        loop {
            tokio::select! {
                // Handle new NXDOMAIN domains
                Some((domain, source_tag, retry_time)) = rx.recv() => {
                    retry_queue.push((domain, source_tag, retry_time, 0));
                }
                
                // Process retry queue
                _ = sleep(Duration::from_millis(1000)) => {
                    let now = Instant::now();
                    let mut i = 0;
                    
                    while i < retry_queue.len() {
                        let (domain, source_tag, next_retry, attempt) = &retry_queue[i];
                        
                        if now >= *next_retry {
                            let domain = domain.clone();
                            let source_tag = source_tag.clone();
                            let attempt = *attempt;
                            
                            // Remove from queue before processing
                            retry_queue.remove(i);
                            
                            match resolver.resolve(&domain).await {
                                Ok(_dns_info) => {
                                    // Domain now resolves! This should trigger a "resolved_after_nxdomain" alert
                                    log::info!("Domain {} (tag: {}) now resolves after NXDOMAIN", domain, source_tag);
                                    // TODO: Send alert with resolved_after_nxdomain: true
                                    // This will be implemented when we wire everything together in main.rs
                                }
                                Err(e) => {
                                    if e.to_string().contains("NXDOMAIN") && attempt < config.nxdomain_retries {
                                        // Schedule next retry with exponential backoff
                                        let backoff_ms = config.nxdomain_initial_backoff_ms * 2_u64.pow(attempt + 1);
                                        let next_retry = now + Duration::from_millis(backoff_ms);
                                        retry_queue.push((domain, source_tag, next_retry, attempt + 1));
                                    } else {
                                        // Give up after max retries or non-NXDOMAIN error
                                        log::debug!("Giving up on NXDOMAIN retry for {} after {} attempts", domain, attempt + 1);
                                    }
                                }
                            }
                        } else {
                            i += 1;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Fake DNS resolver for testing
    pub struct FakeDnsResolver {
        responses: Arc<Mutex<HashMap<String, Result<DnsInfo, String>>>>,
        call_count: Arc<Mutex<HashMap<String, u32>>>,
    }

    impl FakeDnsResolver {
        pub fn new() -> Self {
            Self {
                responses: Arc::new(Mutex::new(HashMap::new())),
                call_count: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        /// Configure the resolver to return a successful response for a domain
        pub fn set_success_response(&self, domain: &str, dns_info: DnsInfo) {
            let mut responses = self.responses.lock().unwrap();
            responses.insert(domain.to_string(), Ok(dns_info));
        }

        /// Configure the resolver to return an error for a domain
        pub fn set_error_response(&self, domain: &str, error: &str) {
            let mut responses = self.responses.lock().unwrap();
            responses.insert(domain.to_string(), Err(error.to_string()));
        }

        /// Get the number of times a domain was queried
        pub fn get_call_count(&self, domain: &str) -> u32 {
            let call_count = self.call_count.lock().unwrap();
            call_count.get(domain).copied().unwrap_or(0)
        }
    }

    #[async_trait]
    impl DnsResolver for FakeDnsResolver {
        async fn resolve(&self, domain: &str) -> Result<DnsInfo> {
            // Increment call count
            {
                let mut call_count = self.call_count.lock().unwrap();
                *call_count.entry(domain.to_string()).or_insert(0) += 1;
            }

            // Return configured response
            let responses = self.responses.lock().unwrap();
            match responses.get(domain) {
                Some(Ok(dns_info)) => Ok(dns_info.clone()),
                Some(Err(error)) => Err(anyhow!("{}", error)),
                None => Err(anyhow!("No response configured for {}", domain)),
            }
        }
    }

    #[tokio::test]
    async fn test_fake_dns_resolver_success() {
        let resolver = FakeDnsResolver::new();
        let mut dns_info = DnsInfo::default();
        dns_info.a_records.push("1.2.3.4".parse().unwrap());
        
        resolver.set_success_response("example.com", dns_info.clone());
        
        let result = resolver.resolve("example.com").await.unwrap();
        assert_eq!(result.a_records, dns_info.a_records);
        assert_eq!(resolver.get_call_count("example.com"), 1);
    }

    #[tokio::test]
    async fn test_fake_dns_resolver_error() {
        let resolver = FakeDnsResolver::new();
        resolver.set_error_response("nonexistent.com", "NXDOMAIN");
        
        let result = resolver.resolve("nonexistent.com").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NXDOMAIN"));
        assert_eq!(resolver.get_call_count("nonexistent.com"), 1);
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_standard_retry() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsRetryConfig {
            standard_retries: 2,
            standard_initial_backoff_ms: 10, // Short delay for testing
            ..Default::default()
        };
        
        let manager = DnsResolutionManager::new(fake_resolver.clone(), config);
        
        // Configure resolver to fail twice, then succeed
        fake_resolver.set_error_response("flaky.com", "Timeout");
        
        // Start the resolution attempt
        let result = manager.resolve_with_retry("flaky.com", "test").await;
        
        // Should fail after retries
        assert!(result.is_err());
        assert_eq!(fake_resolver.get_call_count("flaky.com"), 3); // Initial + 2 retries
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_nxdomain_immediate_error() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsRetryConfig::default();
        
        let manager = DnsResolutionManager::new(fake_resolver.clone(), config);
        
        // Configure resolver to return NXDOMAIN
        fake_resolver.set_error_response("nonexistent.com", "NXDOMAIN");
        
        let result = manager.resolve_with_retry("nonexistent.com", "test").await;
        
        // Should return NXDOMAIN error immediately (no standard retries)
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NXDOMAIN"));
        assert_eq!(fake_resolver.get_call_count("nonexistent.com"), 1);
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_success() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsRetryConfig::default();
        
        let manager = DnsResolutionManager::new(fake_resolver.clone(), config);
        
        let mut dns_info = DnsInfo::default();
        dns_info.a_records.push("1.2.3.4".parse().unwrap());
        fake_resolver.set_success_response("example.com", dns_info.clone());
        
        let result = manager.resolve_with_retry("example.com", "test").await.unwrap();
        
        assert_eq!(result.a_records, dns_info.a_records);
        assert_eq!(fake_resolver.get_call_count("example.com"), 1);
    }
}
