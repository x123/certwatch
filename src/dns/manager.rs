use crate::{
    core::{DnsInfo, DnsResolver},
    dns::{DnsError, DnsHealthMonitor, DnsRetryConfig},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::{sleep, Instant},
};
use tracing::{debug, error, info, instrument, warn};

/// Represents a domain that has resolved after previously being NXDOMAIN
pub type ResolvedNxDomain = (String, String, DnsInfo); // (domain, source_tag, dns_info)

/// Checks if an `anyhow::Error` is an NXDOMAIN error.
fn is_nxdomain_error_str(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("nxdomain") || lower.contains("no records found")
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

        // Start the heartbeat task, but not during tests, as it interferes with paused time.
        #[cfg(not(test))]
        {
            let hb_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                crate::utils::heartbeat::run_heartbeat("DnsResolutionManager", hb_shutdown_rx)
                    .await;
            });
        }

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
    #[instrument(skip(self), fields(domain = %domain, source_tag = %source_tag))]
    pub async fn resolve_with_retry(
        &self,
        domain: &str,
        source_tag: &str,
    ) -> Result<DnsInfo, DnsError> {
        let retries = self.config.retries.unwrap_or(3);
        let backoff_ms = self.config.backoff_ms.unwrap_or(500);
        let nxdomain_backoff_ms = self.config.nxdomain_backoff_ms.unwrap_or(5000);
        debug!(
            retries,
            backoff_ms,
            nxdomain_backoff_ms,
            "Using DNS retry config"
        );

        let mut last_error = None;
        let mut shutdown_rx = self.shutdown_rx.clone();

        for attempt in 0..=retries {
            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {
                    debug!("DNS resolution cancelled by shutdown signal");
                    return Err(DnsError::Shutdown);
                }

                result = self.resolver.resolve(domain) => {
                    match result {
                        Ok(dns_info) => {
                            self.health_monitor.record_outcome(true);
                            metrics::counter!("dns_resolutions", "status" => "success").increment(1);
                            return Ok(dns_info);
                        }
                        Err(DnsError::Resolution(e)) => {
                            self.health_monitor.record_outcome(false);
                            debug!(error = %e, "Handling resolution error");

                            // Check if this is an NXDOMAIN error
                            if is_nxdomain_error_str(&e) {
                                debug!("Detected NXDOMAIN");
                                metrics::counter!("dns_resolutions", "status" => "nxdomain").increment(1);

                                // Schedule for NXDOMAIN retry queue
                                let retry_time = Instant::now()
                                    + Duration::from_millis(nxdomain_backoff_ms);
                                if let Err(send_err) = self.nxdomain_retry_tx.send((
                                    domain.to_string(),
                                    source_tag.to_string(),
                                    retry_time,
                                )) {
                                    error!(error = %send_err, "Failed to schedule NXDOMAIN retry");
                                }

                                // Return the NXDOMAIN error immediately for immediate alert generation
                                return Err(DnsError::Resolution(e));
                            }

                            if e.to_lowercase().contains("timeout") {
                                metrics::counter!("dns_resolutions", "status" => "timeout").increment(1);
                            } else {
                                metrics::counter!("dns_resolutions", "status" => "failure").increment(1);
                            }
                            last_error = Some(e);

                            // For standard errors, apply exponential backoff
                            if attempt < retries {
                                let backoff_duration = backoff_ms * 2_u64.pow(attempt);
                                debug!(backoff_ms = backoff_duration, "Retrying after backoff");
                                sleep(Duration::from_millis(backoff_duration)).await;
                            }
                        }
                        Err(DnsError::Shutdown) => {
                            // This should not happen as the resolver itself doesn't know about shutdown
                            warn!("Resolver unexpectedly returned a shutdown error");
                            return Err(DnsError::Shutdown);
                        }
                    }
                }
            }
        }

        Err(DnsError::Resolution(last_error.unwrap_or_else(|| {
            format!(
                "DNS resolution failed after {} retries",
                retries
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

        let nxdomain_retries = config.nxdomain_retries.unwrap_or(10);
        let nxdomain_backoff_ms = config.nxdomain_backoff_ms.unwrap_or(5000);
        debug!(
            nxdomain_retries,
            nxdomain_backoff_ms,
            "Starting NXDOMAIN retry task"
        );

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
                    info!("NXDOMAIN retry task received shutdown signal.");
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
                                    info!(%domain, %source_tag, "Domain now resolves after NXDOMAIN");
                                    if let Err(e) = resolved_tx.send((domain, source_tag, dns_info)) {
                                        error!(error = %e, "Failed to send resolved NXDOMAIN to channel");
                                    }
                                }
                                Err(e) => {
                                    if let DnsError::Resolution(res_err) = e {
                                       if is_nxdomain_error_str(&res_err) && attempt < nxdomain_retries {
                                            let backoff_ms = nxdomain_backoff_ms * 2_u64.pow(attempt + 1);
                                            let next_retry = now + Duration::from_millis(backoff_ms);
                                            retry_heap.push((Reverse(next_retry), domain, source_tag, attempt + 1));
                                            metrics::gauge!("nxdomain_retry_queue_size").set(retry_heap.len() as f64);
                                       } else {
                                            debug!(error = %res_err, attempt, "Giving up on NXDOMAIN retry");
                                       }
                                    } else {
                                        debug!(error = %e, attempt, "Giving up on NXDOMAIN retry");
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