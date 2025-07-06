//! DNS resolution service with dual-curve retry logic
//!
//! This module implements DNS resolution with separate retry strategies
//! for standard failures and NXDOMAIN responses.

pub mod health;

use crate::core::{DnsInfo, DnsResolver};
use crate::config::DnsConfig;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
pub use health::DnsHealthMonitor;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};
use trust_dns_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    system_conf, TokioAsyncResolver,
};

/// Configuration for DNS retry policies
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// Creates a new DNS resolver from the application's DNS configuration.
    pub fn from_config(config: &DnsConfig) -> Result<(Self, Vec<SocketAddr>)> {
        let (resolver_config, nameservers) = if let Some(resolver_addr_str) = &config.resolver {
            // If a specific resolver is provided, use it exclusively.
            let mut custom_config = ResolverConfig::new();
            let socket_addr: SocketAddr = resolver_addr_str.parse()?;
            custom_config.add_name_server(NameServerConfig::new(socket_addr, Protocol::Udp));
            (custom_config, vec![socket_addr])
        } else {
            // Otherwise, try to get the system's nameservers, but nothing else.
            let (system_config, _) = system_conf::read_system_conf()?;
            let mut clean_config = ResolverConfig::new();
            let mut nameservers = Vec::new();

            if system_config.name_servers().is_empty() {
                log::warn!("No system DNS servers found, falling back to Cloudflare DNS.");
                clean_config = ResolverConfig::cloudflare();
                // We know what Cloudflare's IPs are
                nameservers.push("1.1.1.1:53".parse()?);
                nameservers.push("1.0.0.1:53".parse()?);
            } else {
                for ns in system_config.name_servers() {
                    // Only add UDP servers to avoid duplicates in logging
                    if ns.protocol == Protocol::Udp {
                        clean_config.add_name_server(ns.clone());
                        nameservers.push(ns.socket_addr);
                    }
                }
                // If no UDP servers were found, add all to be safe
                if nameservers.is_empty() {
                    for ns in system_config.name_servers() {
                        clean_config.add_name_server(ns.clone());
                        nameservers.push(ns.socket_addr);
                    }
                }
            }
            (clean_config, nameservers)
        };

        let mut resolver_opts = ResolverOpts::default();
        // Set ndots to 1 to prevent the resolver from appending local search domains.
        // This ensures that we are always resolving the FQDN from the cert stream.
        resolver_opts.ndots = 1;

        // Override the timeout if specified in our application config.
        if let Some(timeout_ms) = config.timeout_ms {
            resolver_opts.timeout = Duration::from_millis(timeout_ms);
        }

        let resolver = TokioAsyncResolver::tokio(resolver_config, resolver_opts);
        Ok((Self { resolver }, nameservers))
    }
}

#[async_trait]
impl DnsResolver for TrustDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo> {
        let mut dns_info = DnsInfo::default();
        let mut first_error = None;

        // Resolve A records (IPv4)
        if let Err(e) = self.resolver.ipv4_lookup(domain).await.map(|lookup| {
            dns_info.a_records = lookup.iter().map(|ip| IpAddr::V4(ip.0)).collect();
        }) {
            log::warn!("Failed to resolve A records for {}: {}", domain, e);
            if first_error.is_none() {
                first_error = Some(e.into());
            }
        }

        // Resolve AAAA records (IPv6)
        if let Err(e) = self.resolver.ipv6_lookup(domain).await.map(|lookup| {
            dns_info.aaaa_records = lookup.iter().map(|ip| IpAddr::V6(ip.0)).collect();
        }) {
            log::warn!("Failed to resolve AAAA records for {}: {}", domain, e);
            if first_error.is_none() {
                first_error = Some(e.into());
            }
        }

        // Resolve NS records
        if let Err(e) = self.resolver.ns_lookup(domain).await.map(|lookup| {
            dns_info.ns_records = lookup.iter().map(|ns| ns.to_string()).collect();
        }) {
            log::warn!("Failed to resolve NS records for {}: {}", domain, e);
            if first_error.is_none() {
                first_error = Some(e.into());
            }
        }

        // If we have a specific error, and it's an NXDOMAIN, return it immediately.
        if let Some(err) = &first_error {
            if is_nxdomain_error(err) {
                return Err(first_error.unwrap());
            }
        }

        // If we got no records at all, return the first specific error we encountered.
        if dns_info.is_empty() {
            return Err(first_error
                .unwrap_or_else(|| anyhow!("No DNS records found for {}", domain)));
        }

        Ok(dns_info)
    }
}

/// Represents a domain that has resolved after previously being NXDOMAIN
pub type ResolvedNxDomain = (String, String, DnsInfo); // (domain, source_tag, dns_info)

/// Checks if an `anyhow::Error` is an NXDOMAIN error.
fn is_nxdomain_error(err: &anyhow::Error) -> bool {
    if let Some(proto_err) = err.downcast_ref::<trust_dns_resolver::error::ResolveError>() {
        if let trust_dns_resolver::error::ResolveErrorKind::NoRecordsFound { response_code, .. } =
            proto_err.kind()
        {
            return *response_code == trust_dns_resolver::proto::op::ResponseCode::NXDomain;
        }
    }
    false
}

/// Manages DNS resolution with dual-curve retry logic
pub struct DnsResolutionManager {
    resolver: Arc<dyn DnsResolver>,
    config: DnsRetryConfig,
    nxdomain_retry_tx: mpsc::UnboundedSender<(String, String, Instant)>, // (domain, source_tag, retry_time)
    health_monitor: Arc<DnsHealthMonitor>,
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager
    ///
    /// Returns the manager and a receiver for domains that resolve after being NXDOMAIN
    pub fn new(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        health_monitor: Arc<DnsHealthMonitor>,
    ) -> (Self, mpsc::UnboundedReceiver<ResolvedNxDomain>) {
        let (nxdomain_retry_tx, nxdomain_retry_rx) = mpsc::unbounded_channel();
        let (resolved_nxdomain_tx, resolved_nxdomain_rx) = mpsc::unbounded_channel();

        // Spawn the NXDOMAIN retry task
        let resolver_clone = Arc::clone(&resolver);
        let config_clone = config.clone();
        tokio::spawn(Self::nxdomain_retry_task(
            resolver_clone,
            config_clone,
            nxdomain_retry_rx,
            resolved_nxdomain_tx,
        ));

        let manager = Self {
            resolver,
            config,
            nxdomain_retry_tx,
            health_monitor,
        };

        (manager, resolved_nxdomain_rx)
    }

    /// Attempts to resolve a domain with standard retry logic
    pub async fn resolve_with_retry(&self, domain: &str, source_tag: &str) -> Result<DnsInfo> {
        let mut last_error = None;
        
        for attempt in 0..=self.config.standard_retries {
            match self.resolver.resolve(domain).await {
                Ok(dns_info) => {
                    self.health_monitor.record_outcome(true);
                    metrics::counter!("dns_resolutions", "status" => "success").increment(1);
                    return Ok(dns_info);
                }
                Err(e) => {
                    self.health_monitor.record_outcome(false);

                    // Check if this is an NXDOMAIN error
                    if is_nxdomain_error(&e) {
                        metrics::counter!("dns_resolutions", "status" => "nxdomain").increment(1);

                        // Schedule for NXDOMAIN retry queue
                        let retry_time = Instant::now()
                            + Duration::from_millis(self.config.nxdomain_initial_backoff_ms);
                        if let Err(send_err) = self.nxdomain_retry_tx.send((
                            domain.to_string(),
                            source_tag.to_string(),
                            retry_time,
                        )) {
                            log::error!("Failed to schedule NXDOMAIN retry: {}", send_err);
                        }

                        // Return the NXDOMAIN error immediately for immediate alert generation
                        return Err(e);
                    }

                    let error_str = e.to_string();
                    if error_str.to_lowercase().contains("timeout") {
                        metrics::counter!("dns_resolutions", "status" => "timeout").increment(1);
                    } else {
                        metrics::counter!("dns_resolutions", "status" => "failure").increment(1);
                    }
                    last_error = Some(error_str);
                    
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
        resolved_tx: mpsc::UnboundedSender<ResolvedNxDomain>,
    ) {
        // Use a min-heap to keep the next retry item at the top.
        // We store (Reverse(next_retry_time), domain, source_tag, attempt)
        // Reverse is used to make the std::collections::BinaryHeap a min-heap.
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        type HeapItem = (Reverse<Instant>, String, String, u32);
        let mut retry_heap: BinaryHeap<HeapItem> = BinaryHeap::new();

        loop {
            metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);

            // Determine the sleep duration. If the heap is empty, wait indefinitely
            // for a new message. Otherwise, sleep until the next retry is due.
            let sleep_duration = if let Some(Reverse(next_retry_time)) = retry_heap.peek().map(|(t, ..)| *t) {
                let now = Instant::now();
                if now >= next_retry_time {
                    Duration::from_millis(0) // No sleep, process immediately
                } else {
                    next_retry_time - now
                }
            } else {
                Duration::from_secs(3600) // Effectively, wait for a new item
            };

            tokio::select! {
                // biased; always prefer receiving a new item over sleeping
                biased;

                // Handle new NXDOMAIN domains
                Some((domain, source_tag, retry_time)) = rx.recv() => {
                    retry_heap.push((Reverse(retry_time), domain, source_tag, 0));
                    metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);
                },

                // Process retry queue
                _ = sleep(sleep_duration) => {
                    let now = Instant::now();
                    while let Some(Reverse(next_retry_time)) = retry_heap.peek().map(|(t, ..)| *t) {
                        if now < next_retry_time {
                            break; // Not time yet for the next item
                        }

                        // It's time, pop the item from the heap
                        if let Some((_, domain, source_tag, attempt)) = retry_heap.pop() {
                            metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);
                            match resolver.resolve(&domain).await {
                                Ok(dns_info) => {
                                    log::info!("Domain {} (tag: {}) now resolves after NXDOMAIN", domain, source_tag);
                                    if let Err(e) = resolved_tx.send((domain, source_tag, dns_info)) {
                                        log::error!("Failed to send resolved NXDOMAIN to channel: {}", e);
                                    }
                                }
                                Err(e) => {
                                    if e.to_string().contains("NXDOMAIN") && attempt < config.nxdomain_retries {
                                        let backoff_ms = config.nxdomain_initial_backoff_ms * 2_u64.pow(attempt + 1);
                                        let next_retry = now + Duration::from_millis(backoff_ms);
                                        retry_heap.push((Reverse(next_retry), domain, source_tag, attempt + 1));
                                        metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);
                                    } else {
                                        log::debug!("Giving up on NXDOMAIN retry for {} after {} attempts", domain, attempt + 1);
                                    }
                                }
                            }
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
    use trust_dns_resolver::error::{ResolveError, ResolveErrorKind};
    use trust_dns_resolver::proto::op::ResponseCode;

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
                Some(Err(error)) => {
                    if error == "NXDOMAIN" {
                        let kind = ResolveErrorKind::NoRecordsFound {
                            query: Default::default(),
                            response_code: ResponseCode::NXDomain,
                            trusted: false,
                            negative_ttl: None,
                            soa: None,
                        };
                        Err(ResolveError::from(kind).into())
                    } else {
                        Err(anyhow!("{}", error))
                    }
                }
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
        assert!(is_nxdomain_error(&result.unwrap_err()));
        assert_eq!(resolver.get_call_count("nonexistent.com"), 1);
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_standard_retry() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let retry_config = DnsRetryConfig {
            standard_retries: 2,
            standard_initial_backoff_ms: 10, // Short delay for testing
            ..Default::default()
        };
        let config = DnsConfig {
            retry_config,
            ..Default::default()
        };

        let health_monitor = DnsHealthMonitor::new(Default::default(), fake_resolver.clone());
        let (manager, _) =
            DnsResolutionManager::new(fake_resolver.clone(), config.retry_config, health_monitor);

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
        let config = DnsConfig::default();

        let health_monitor = DnsHealthMonitor::new(Default::default(), fake_resolver.clone());
        let (manager, _) =
            DnsResolutionManager::new(fake_resolver.clone(), config.retry_config, health_monitor);

        // Configure resolver to return NXDOMAIN
        fake_resolver.set_error_response("nonexistent.com", "NXDOMAIN");

        let result = manager.resolve_with_retry("nonexistent.com", "test").await;

        // Should return NXDOMAIN error immediately (no standard retries)
        assert!(result.is_err());
        assert!(is_nxdomain_error(&result.unwrap_err()));
        assert_eq!(fake_resolver.get_call_count("nonexistent.com"), 1);
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_success() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsConfig::default();

        let health_monitor = DnsHealthMonitor::new(Default::default(), fake_resolver.clone());
        let (manager, _) =
            DnsResolutionManager::new(fake_resolver.clone(), config.retry_config, health_monitor);

        let mut dns_info = DnsInfo::default();
        dns_info.a_records.push("1.2.3.4".parse().unwrap());
        fake_resolver.set_success_response("example.com", dns_info.clone());

        let result = manager.resolve_with_retry("example.com", "test").await.unwrap();

        assert_eq!(result.a_records, dns_info.a_records);
        assert_eq!(fake_resolver.get_call_count("example.com"), 1);
    }

    #[tokio::test]
    async fn test_nxdomain_retry_task_sends_resolved_domain() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let retry_config = DnsRetryConfig {
            nxdomain_retries: 1,
            nxdomain_initial_backoff_ms: 10, // Short delay for testing
            ..Default::default()
        };
        let config = DnsConfig {
            retry_config,
            ..Default::default()
        };

        let health_monitor = DnsHealthMonitor::new(Default::default(), fake_resolver.clone());
        let (manager, mut resolved_rx) = DnsResolutionManager::new(
            fake_resolver.clone(),
            config.retry_config,
            health_monitor,
        );

        // Configure resolver to return NXDOMAIN initially
        fake_resolver.set_error_response("newly-active.com", "NXDOMAIN");

        // This first call should fail immediately and queue the domain for retry
        let result = manager.resolve_with_retry("newly-active.com", "test-tag").await;
        assert!(result.is_err());
        assert!(is_nxdomain_error(&result.unwrap_err()));
        assert_eq!(fake_resolver.get_call_count("newly-active.com"), 1);

        // Now, configure the resolver to return a success response
        let mut success_dns_info = DnsInfo::default();
        success_dns_info.a_records.push("5.5.5.5".parse().unwrap());
        fake_resolver.set_success_response("newly-active.com", success_dns_info.clone());

        // Wait for the retry task to process the domain and send it to the resolved channel
        let resolved_result = tokio::time::timeout(Duration::from_secs(1), resolved_rx.recv()).await;
        assert!(resolved_result.is_ok(), "Did not receive resolved domain from channel");

        let (domain, tag, dns_info) = resolved_result.unwrap().unwrap();
        assert_eq!(domain, "newly-active.com");
        assert_eq!(tag, "test-tag");
        assert_eq!(dns_info.a_records, success_dns_info.a_records);

        // The resolver should have been called a second time
        assert_eq!(fake_resolver.get_call_count("newly-active.com"), 2);
    }
}
