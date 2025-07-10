use crate::{
    core::{DnsInfo, DnsResolver},
    dns::{DnsError, DnsHealthMonitor, DnsRetryConfig},
    internal_metrics::Metrics,
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
    dns_request_tx: mpsc::Sender<(String, String)>, // (domain, source_tag)
    _shutdown_rx: watch::Receiver<()>, // Kept for ownership, not direct use
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager and starts its background processing task.
    ///
    /// Returns the manager and a receiver for domains that resolve after being NXDOMAIN.
    pub fn start(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        health_monitor: Arc<DnsHealthMonitor>,
        shutdown_rx: watch::Receiver<()>,
        _metrics: Arc<Metrics>,
    ) -> (Self, mpsc::UnboundedReceiver<ResolvedNxDomain>) {
        let (nxdomain_retry_tx, nxdomain_retry_rx) = mpsc::unbounded_channel();
        let (resolved_nxdomain_tx, resolved_nxdomain_rx) = mpsc::unbounded_channel();
        let (dns_request_tx, dns_request_rx) = mpsc::channel(10_000);

        // Spawn the primary resolution task
        let resolver_clone = Arc::clone(&resolver);
        let config_clone = config.clone();
        let mut task_shutdown_rx = shutdown_rx.clone();
        let task_health_monitor = health_monitor.clone();
        tokio::spawn(async move {
            Self::resolution_task(
                resolver_clone,
                config_clone,
                dns_request_rx,
                nxdomain_retry_tx.clone(),
                task_health_monitor,
                &mut task_shutdown_rx,
            )
            .await;
        });

        // Spawn the NXDOMAIN retry task
        let nx_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(Self::nxdomain_retry_task(
            resolver,
            config,
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
            dns_request_tx,
            _shutdown_rx: shutdown_rx,
        };

        (manager, resolved_nxdomain_rx)
    }

    /// Sends a domain to the resolution channel. This is a non-blocking call.
    pub fn resolve(&self, domain: String, source_tag: String) -> Result<(), DnsError> {
        match self.dns_request_tx.try_send((domain, source_tag)) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!("DNS request channel is full. Dropping domain.");
                Err(DnsError::Resolution("DNS channel full".to_string()))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                error!("DNS request channel is closed.");
                Err(DnsError::Shutdown)
            }
        }
    }

    /// The primary background task for handling DNS resolution requests.
    async fn resolution_task(
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        mut dns_request_rx: mpsc::Receiver<(String, String)>,
        nxdomain_retry_tx: mpsc::UnboundedSender<(String, String, Instant)>,
        health_monitor: Arc<DnsHealthMonitor>,
        shutdown_rx: &mut watch::Receiver<()>,
    ) {
        info!("DNS resolution task started.");
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.changed() => {
                    info!("DNS resolution task received shutdown signal.");
                    break;
                }
                Some((domain, source_tag)) = dns_request_rx.recv() => {
                    let resolver_clone = resolver.clone();
                    let nxdomain_retry_tx_clone = nxdomain_retry_tx.clone();
                    let health_monitor_clone = health_monitor.clone();
                    let config_clone = config.clone();

                    // We don't await this handle. This is the key change: resolution
                    // happens in a separate, detached task.
                    tokio::spawn(async move {
                        let _ = Self::perform_resolution(
                            &domain,
                            &source_tag,
                            resolver_clone,
                            config_clone,
                            nxdomain_retry_tx_clone,
                            health_monitor_clone,
                        ).await;
                    });
                }
            }
        }
        info!("DNS resolution task finished.");
    }

    /// Performs the actual resolution with retry logic for a single domain.
    #[instrument(skip_all, fields(domain = %domain, source_tag = %source_tag))]
    async fn perform_resolution(
        domain: &str,
        source_tag: &str,
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        nxdomain_retry_tx: mpsc::UnboundedSender<(String, String, Instant)>,
        health_monitor: Arc<DnsHealthMonitor>,
    ) -> Result<DnsInfo, DnsError> {
        let retries = config.retries.unwrap_or(3);
        let backoff_ms = config.backoff_ms.unwrap_or(500);
        let nxdomain_backoff_ms = config.nxdomain_backoff_ms.unwrap_or(5000);

        let mut last_error = None;

        for attempt in 0..=retries {
            match resolver.resolve(domain).await {
                Ok(dns_info) => {
                    health_monitor.record_outcome(true);
                    metrics::counter!("dns_queries_total", "status" => "success").increment(1);
                    return Ok(dns_info);
                }
                Err(DnsError::Resolution(e)) => {
                    health_monitor.record_outcome(false);
                    metrics::counter!("dns_queries_total", "status" => "failure").increment(1);
                    debug!(error = %e, "Handling resolution error");

                    if is_nxdomain_error_str(&e) {
                        debug!("Detected NXDOMAIN");
                        let retry_time =
                            Instant::now() + Duration::from_millis(nxdomain_backoff_ms);
                        if let Err(send_err) =
                            nxdomain_retry_tx.send((domain.to_string(), source_tag.to_string(), retry_time))
                        {
                            error!(error = %send_err, "Failed to schedule NXDOMAIN retry");
                        }
                        return Err(DnsError::Resolution(e));
                    }

                    last_error = Some(e);

                    if attempt < retries {
                        let backoff_duration = backoff_ms * 2_u64.pow(attempt);
                        debug!(backoff_ms = backoff_duration, "Retrying after backoff");
                        sleep(Duration::from_millis(backoff_duration)).await;
                    }
                }
                Err(DnsError::Shutdown) => {
                    warn!("Resolver unexpectedly returned a shutdown error");
                    return Err(DnsError::Shutdown);
                }
            }
        }

        Err(DnsError::Resolution(
            last_error.unwrap_or_else(|| format!("DNS resolution failed after {} retries", retries)),
        ))
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
                            match resolver.resolve(&domain).await {
                                Ok(dns_info) => {
                                    info!(%domain, %source_tag, "Domain now resolves after NXDOMAIN");
                                    if let Err(e) = resolved_tx.send((domain, source_tag, dns_info)) {
                                        error!(error = %e, "Failed to send resolved NXDOMAIN to channel");
                                    }
                                }
                                Err(e) => {
                                    if let DnsError::Resolution(res_err) = e {
                                        if is_nxdomain_error_str(&res_err)
                                            && attempt < nxdomain_retries
                                        {
                                            let backoff_ms =
                                                nxdomain_backoff_ms * 2_u64.pow(attempt + 1);
                                            let next_retry = now + Duration::from_millis(backoff_ms);
                                            retry_heap.push((
                                                Reverse(next_retry),
                                                domain,
                                                source_tag,
                                                attempt + 1,
                                            ));
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