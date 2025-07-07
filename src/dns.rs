use metrics;
// DNS resolution service with dual-curve retry logic
//
// This module implements DNS resolution with separate retry strategies
// for standard failures and NXDOMAIN responses.

pub mod health;

use crate::core::DnsInfo;
pub use crate::core::DnsResolver;
use crate::config::DnsConfig;
use crate::utils::heartbeat::run_heartbeat;
use anyhow::Result;
use async_trait::async_trait;
pub use health::DnsHealthMonitor;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, Instant};
use hickory_resolver::{
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    proto::xfer::Protocol,
    system_conf, TokioResolver,
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

#[derive(Error, Debug, Clone)]
pub enum DnsError {
    #[error("DNS resolution failed: {0}")]
    Resolution(String),

    #[error("DNS resolution cancelled due to shutdown")]
    Shutdown,
}

/// DNS resolver implementation using trust-dns-resolver
pub struct HickoryDnsResolver {
    resolver: TokioResolver,
}

impl HickoryDnsResolver {
    /// Creates a new DNS resolver from the application's DNS configuration.
    pub fn from_config(config: &DnsConfig) -> Result<(Self, Vec<SocketAddr>)> {
        let resolver_config = if let Some(resolver_addr_str) = &config.resolver {
                // If a specific resolver is provided, use it exclusively.
                let mut custom_config = ResolverConfig::new();
                let socket_addr: SocketAddr = resolver_addr_str.parse()?;
                custom_config
                    .add_name_server(NameServerConfig::new(socket_addr, Protocol::Udp));
                custom_config
            } else {
                // Otherwise, load from system config
                let (system_config, _) = system_conf::read_system_conf()?;
                if system_config.name_servers().is_empty() {
                    log::warn!("No system DNS servers found, falling back to Cloudflare DNS.");
                    ResolverConfig::cloudflare()
                } else {
                    system_config
                }
            };

        let mut resolver_config_with_no_search = ResolverConfig::new();
        for ns in resolver_config.name_servers() {
            resolver_config_with_no_search.add_name_server(ns.clone());
        }

        let mut nameservers: Vec<_> = resolver_config_with_no_search
            .name_servers()
            .iter()
            .map(|ns| ns.socket_addr)
            .collect();
        nameservers.sort();
        nameservers.dedup();

        let mut resolver_opts = ResolverOpts::default();
        // Set ndots to 1 to prevent the resolver from appending local search domains.
        // This ensures that we are always resolving the FQDN from the cert stream.
        resolver_opts.ndots = 1;

        // Override the timeout if specified in our application config.
        if let Some(timeout_ms) = config.timeout_ms {
            resolver_opts.timeout = Duration::from_millis(timeout_ms);
        }

        let resolver = hickory_resolver::Resolver::builder_with_config(
            resolver_config_with_no_search,
            hickory_resolver::name_server::TokioConnectionProvider::default(),
        )
        .with_options(resolver_opts)
        .build();

        Ok((Self { resolver }, nameservers))
    }
}

#[async_trait]
impl DnsResolver for HickoryDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        use hickory_resolver::proto::rr::RecordType;

        // Perform concurrent lookups for A, AAAA, and NS records
        let (a_result, aaaa_result, ns_result) = tokio::join!(
            self.resolver.lookup(domain, RecordType::A),
            self.resolver.lookup(domain, RecordType::AAAA),
            self.resolver.lookup(domain, RecordType::NS)
        );

        let mut dns_info = DnsInfo::default();
        let mut first_error = None;

        // Process A records
        match a_result {
            Ok(lookup) => {
                dns_info.a_records = lookup.into_iter().filter_map(|r| r.ip_addr()).collect();
            }
            Err(e) => {
                log::debug!("A record lookup failed for {}: {}", domain, e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        // Process AAAA records
        match aaaa_result {
            Ok(lookup) => {
                dns_info.aaaa_records = lookup.into_iter().filter_map(|r| r.ip_addr()).collect();
            }
            Err(e) => {
                log::debug!("AAAA record lookup failed for {}: {}", domain, e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        // Process NS records
        match ns_result {
            Ok(lookup) => {
                dns_info.ns_records = lookup.into_iter().map(|r| r.to_string()).collect();
            }
            Err(e) => {
                log::debug!("NS record lookup failed for {}: {}", domain, e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        // Only return an error if all lookups failed.
        if dns_info.is_empty() {
            if let Some(err) = first_error {
                let err_string = err.to_string();
                if is_nxdomain_error_str(&err_string) {
                    return Err(DnsError::Resolution(err_string));
                }
                return Err(DnsError::Resolution(format!(
                    "All DNS lookups failed for {}: {}",
                    domain, err_string
                )));
            } else {
                // This case should be rare, but it's possible if all lookups succeed but return no records.
                return Err(DnsError::Resolution(format!(
                    "No A, AAAA, or NS records found for {}",
                    domain
                )));
            }
        }

        Ok(dns_info)
    }
}

/// Represents a domain that has resolved after previously being NXDOMAIN
pub type ResolvedNxDomain = (String, String, DnsInfo); // (domain, source_tag, dns_info)

/// Checks if an `anyhow::Error` is an NXDOMAIN error.
fn is_nxdomain_error_str(err_str: &str) -> bool {
    err_str.contains("NXDOMAIN")
}

/// Manages DNS resolution with dual-curve retry logic
pub struct DnsResolutionManager {
    resolver: Arc<dyn DnsResolver>,
    config: DnsRetryConfig,
    nxdomain_retry_tx: mpsc::UnboundedSender<(String, String, Instant)>, // (domain, source_tag, retry_time)
    health_monitor: Arc<DnsHealthMonitor>,
    shutdown_rx: watch::Receiver<()>,
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager
    ///
    /// Returns the manager and a receiver for domains that resolve after being NXDOMAIN
    pub fn new(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        health_monitor: Arc<DnsHealthMonitor>,
        shutdown_rx: watch::Receiver<()>,
    ) -> (Self, mpsc::UnboundedReceiver<ResolvedNxDomain>) {
        let (nxdomain_retry_tx, nxdomain_retry_rx) = mpsc::unbounded_channel();
        let (resolved_nxdomain_tx, resolved_nxdomain_rx) = mpsc::unbounded_channel();

        // Spawn the NXDOMAIN retry task
        let resolver_clone = Arc::clone(&resolver);
        let config_clone = config.clone();
        let nx_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(Self::nxdomain_retry_task(
            resolver_clone,
            config_clone,
            nxdomain_retry_rx,
            resolved_nxdomain_tx,
            nx_shutdown_rx,
        ));

        // Start the heartbeat task
        let hb_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            run_heartbeat("DnsResolutionManager", hb_shutdown_rx).await;
        });

        let manager = Self {
            resolver,
            config,
            nxdomain_retry_tx,
            health_monitor,
            shutdown_rx,
        };

        (manager, resolved_nxdomain_rx)
    }

    /// Attempts to resolve a domain with standard retry logic
    pub async fn resolve_with_retry(
        &self,
        domain: &str,
        source_tag: &str,
    ) -> Result<DnsInfo, DnsError> {
        let mut last_error = None;
        let mut shutdown_rx = self.shutdown_rx.clone();

        for attempt in 0..=self.config.standard_retries {
            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {
                    log::debug!("DNS resolution for {} cancelled by shutdown signal", domain);
                    return Err(DnsError::Shutdown);
                }

                result = self.resolver.resolve(domain) => {
                    log::info!("DnsResolutionManager: Received result for {}: {:?}", domain, result);
                    match result {
                        Ok(dns_info) => {
                            self.health_monitor.record_outcome(true);
                            metrics::counter!("dns_resolutions", "status" => "success").increment(1);
                            return Ok(dns_info);
                        }
                        Err(DnsError::Resolution(e)) => {
                            self.health_monitor.record_outcome(false);
                            log::info!("DnsResolutionManager: Handling resolution error for {}: {}", domain, e);

                            // Check if this is an NXDOMAIN error
                            if is_nxdomain_error_str(&e) {
                                log::info!("DnsResolutionManager: Detected NXDOMAIN for {}", domain);
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
                                return Err(DnsError::Resolution(e));
                            }

                            log::info!("DnsResolutionManager: Standard failure for {}: {}", domain, e);
                            if e.to_lowercase().contains("timeout") {
                                metrics::counter!("dns_resolutions", "status" => "timeout").increment(1);
                            } else {
                                metrics::counter!("dns_resolutions", "status" => "failure").increment(1);
                            }
                            last_error = Some(e);

                            // For standard errors, apply exponential backoff
                            if attempt < self.config.standard_retries {
                                let backoff_ms = self.config.standard_initial_backoff_ms * 2_u64.pow(attempt);
                                log::info!("DnsResolutionManager: Retrying {} in {}ms", domain, backoff_ms);
                                sleep(Duration::from_millis(backoff_ms)).await;
                            }
                        }
                        Err(DnsError::Shutdown) => {
                            // This should not happen as the resolver itself doesn't know about shutdown
                            log::warn!("Resolver unexpectedly returned a shutdown error");
                            return Err(DnsError::Shutdown);
                        }
                    }
                }
            }
        }

        Err(DnsError::Resolution(last_error.unwrap_or_else(|| {
            format!(
                "DNS resolution failed after {} retries",
                self.config.standard_retries
            )
        })))
    }

    /// Background task that handles NXDOMAIN retries
    async fn nxdomain_retry_task(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        mut rx: mpsc::UnboundedReceiver<(String, String, Instant)>,
        resolved_tx: mpsc::UnboundedSender<ResolvedNxDomain>,
        mut shutdown_rx: tokio::sync::watch::Receiver<()>,
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
                biased;

                _ = shutdown_rx.changed() => {
                    log::info!("NXDOMAIN retry task received shutdown signal.");
                    break;
                }

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
                                    if let DnsError::Resolution(res_err) = e {
                                       if is_nxdomain_error_str(&res_err) && attempt < config.nxdomain_retries {
                                            let backoff_ms = config.nxdomain_initial_backoff_ms * 2_u64.pow(attempt + 1);
                                            let next_retry = now + Duration::from_millis(backoff_ms);
                                            retry_heap.push((Reverse(next_retry), domain, source_tag, attempt + 1));
                                            metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);
                                       } else {
                                           log::debug!("Giving up on NXDOMAIN retry for {} after {} attempts: {}", domain, attempt + 1, res_err);
                                       }
                                    } else {
                                       log::debug!("Giving up on NXDOMAIN retry for {} after {} attempts: {}", domain, attempt + 1, e);
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

#[cfg(feature = "test-utils")]
pub mod test_utils {
    use super::{DnsError, DnsInfo, DnsResolver};
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

}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::DnsConfig;
    use crate::dns::health::DnsHealthMonitor;
    use crate::dns::test_utils::FakeDnsResolver;
    use std::sync::Arc;
    use tokio::sync::watch;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_fake_dns_resolver_success() {
        let resolver = FakeDnsResolver::new();
        let mut dns_info = DnsInfo::default();
        dns_info.a_records.push("1.2.3.4".parse().unwrap());

        resolver.add_success_response("example.com", dns_info.clone());

        let result = resolver.resolve("example.com").await.unwrap();
        assert_eq!(result.a_records, dns_info.a_records);
        assert_eq!(resolver.get_call_count("example.com"), 1);
    }

    #[tokio::test]
    async fn test_fake_dns_resolver_error() {
        let resolver = FakeDnsResolver::new();
        resolver.add_error_response("nonexistent.com", "NXDOMAIN");

        let result = resolver.resolve("nonexistent.com").await;
        assert!(result.is_err());
        if let Err(DnsError::Resolution(e)) = result {
            assert!(is_nxdomain_error_str(&e));
        } else {
            panic!("Expected a resolution error");
        }
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

        let (_tx, rx) = watch::channel(());
        let health_monitor =
            DnsHealthMonitor::new(Default::default(), fake_resolver.clone(), rx.clone());
        let (manager, _) = DnsResolutionManager::new(
            fake_resolver.clone(),
            config.retry_config,
            health_monitor,
            rx,
        );

        // Configure resolver to fail twice, then succeed
        fake_resolver.add_error_response("flaky.com", "Timeout");
        fake_resolver.add_error_response("flaky.com", "Timeout");
        let mut success_info = DnsInfo::default();
        success_info.a_records.push("1.1.1.1".parse().unwrap());
        fake_resolver.add_success_response("flaky.com", success_info.clone());

        // Start the resolution attempt
        let result = manager.resolve_with_retry("flaky.com", "test").await;

        // Should succeed after 2 retries
        assert!(result.is_ok());
        assert_eq!(result.unwrap().a_records, success_info.a_records);
        assert_eq!(fake_resolver.get_call_count("flaky.com"), 3); // Initial + 2 retries
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_nxdomain_immediate_error() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsConfig::default();

        let (_tx, rx) = watch::channel(());
        let health_monitor =
            DnsHealthMonitor::new(Default::default(), fake_resolver.clone(), rx.clone());
        let (manager, _) = DnsResolutionManager::new(
            fake_resolver.clone(),
            config.retry_config,
            health_monitor,
            rx,
        );

        // Configure resolver to return NXDOMAIN
        fake_resolver.add_error_response("nonexistent.com", "NXDOMAIN");

        let result = manager.resolve_with_retry("nonexistent.com", "test").await;

        // Should return NXDOMAIN error immediately (no standard retries)
        assert!(result.is_err());
        if let Err(DnsError::Resolution(e)) = result {
            assert!(is_nxdomain_error_str(&e));
        } else {
            panic!("Expected a resolution error");
        }
        assert_eq!(fake_resolver.get_call_count("nonexistent.com"), 1);
    }

    #[tokio::test]
    async fn test_dns_resolution_manager_success() {
        let fake_resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsConfig::default();

        let (_tx, rx) = watch::channel(());
        let health_monitor =
            DnsHealthMonitor::new(Default::default(), fake_resolver.clone(), rx.clone());
        let (manager, _) = DnsResolutionManager::new(
            fake_resolver.clone(),
            config.retry_config,
            health_monitor,
            rx,
        );

        let mut dns_info = DnsInfo::default();
        dns_info.a_records.push("1.2.3.4".parse().unwrap());
        fake_resolver.add_success_response("example.com", dns_info.clone());

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

        let (_tx, rx) = watch::channel(());
        let health_monitor =
            DnsHealthMonitor::new(Default::default(), fake_resolver.clone(), rx.clone());
        let (manager, mut resolved_rx) = DnsResolutionManager::new(
            fake_resolver.clone(),
            config.retry_config,
            health_monitor,
            rx,
        );

        // Configure resolver to return NXDOMAIN initially
        fake_resolver.add_error_response("newly-active.com", "NXDOMAIN");

        // This first call should fail immediately and queue the domain for retry
        let result = manager.resolve_with_retry("newly-active.com", "test-tag").await;
        assert!(result.is_err());
        if let Err(DnsError::Resolution(e)) = result {
            assert!(is_nxdomain_error_str(&e));
        } else {
            panic!("Expected a resolution error");
        }
        assert_eq!(fake_resolver.get_call_count("newly-active.com"), 1);

        // Now, configure the resolver to return a success response
        let mut success_dns_info = DnsInfo::default();
        success_dns_info.a_records.push("5.5.5.5".parse().unwrap());
        fake_resolver.add_success_response("newly-active.com", success_dns_info.clone());

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
