use crate::{
    config::PerformanceConfig,
    core::{DnsInfo, DnsResolver},
    dns::{DnsError, DnsHealth, DnsRetryConfig},
    internal_metrics::Metrics,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, watch, Semaphore},
    time::{sleep, Instant},
};
use tracing::{debug, error, info, instrument, warn};

/// Represents a successfully resolved domain.
pub type ResolvedDomain = (String, DnsInfo, Instant); // (domain, dns_info, start_time)

/// Checks if an `anyhow::Error` contains an NXDOMAIN indicator.
fn is_nxdomain_error_str(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("nxdomain") || lower.contains("no records found")
}

/// Manages DNS resolution tasks.
pub struct DnsResolutionManager {
    dns_request_tx: mpsc::Sender<(String, String, Instant)>,
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager and starts its background processing task.
    pub fn start(
        resolver: Arc<dyn DnsResolver>,
        retry_config: DnsRetryConfig,
        performance_config: &PerformanceConfig,
        _health_monitor: Arc<DnsHealth>, // Kept for API compatibility
        shutdown_rx: watch::Receiver<()>,
        metrics: Arc<Metrics>,
    ) -> (Self, async_channel::Receiver<ResolvedDomain>) {
        let (dns_request_tx, dns_request_rx) = mpsc::channel(10_000);
        let (resolved_tx, resolved_rx) =
            async_channel::bounded(performance_config.queue_capacity);

        // Spawn the primary resolution task
        let mut task_shutdown_rx = shutdown_rx.clone();
        let semaphore = Arc::new(Semaphore::new(performance_config.dns_worker_concurrency));

        tokio::spawn(async move {
            Self::resolution_task(
                resolver,
                retry_config,
                dns_request_rx,
                resolved_tx,
                &mut task_shutdown_rx,
                semaphore,
                metrics,
            )
            .await;
        });

        // Start the heartbeat task, but not during tests, as it interferes with paused time.
        #[cfg(not(test))]
        {
            let hb_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                crate::utils::heartbeat::run_heartbeat("DnsResolutionManager", hb_shutdown_rx)
                    .await;
            });
        }

        let manager = Self { dns_request_tx };

        (manager, resolved_rx)
    }

    /// Sends a domain to the resolution channel. This is a non-blocking call.
    pub fn resolve(
        &self,
        domain: String,
        source_tag: String,
        start_time: Instant,
    ) -> Result<(), DnsError> {
        match self.dns_request_tx.try_send((domain, source_tag, start_time)) {
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
        mut dns_request_rx: mpsc::Receiver<(String, String, Instant)>,
        resolved_tx: async_channel::Sender<ResolvedDomain>,
        shutdown_rx: &mut watch::Receiver<()>,
        semaphore: Arc<Semaphore>,
        metrics: Arc<Metrics>,
    ) {
        info!("DNS resolution task started.");
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.changed() => {
                    info!("DNS resolution task received shutdown signal");
                    break;
                }
                Some((domain, _source_tag, start_time)) = dns_request_rx.recv() => {
                    let resolver_clone = resolver.clone();
                    let config_clone = config.clone();
                    let semaphore_clone = semaphore.clone();
                    let resolved_tx_clone = resolved_tx.clone();
                    let metrics_clone = metrics.clone();

                    tokio::spawn(async move {
                        let permit = match semaphore_clone.acquire().await {
                            Ok(p) => p,
                            Err(_) => {
                                warn!("Failed to acquire semaphore permit; semaphore is closed.");
                                return;
                            }
                        };
                        let elapsed = start_time.elapsed();
                        metrics_clone.dns_worker_scheduling_delay_seconds.record(elapsed);


                        if let Ok(dns_info) =
                            Self::perform_resolution(&domain, resolver_clone, config_clone, metrics_clone).await
                        {
                            if resolved_tx_clone
                                .send((domain, dns_info, start_time))
                                .await
                                .is_err()
                            {
                                error!("Failed to send resolved domain to the next stage.");
                            }
                        }
                        // Errors from perform_resolution are logged and handled within the function.
                        // We don't propagate them here, as they are terminal (e.g., final retry failed).

                        drop(permit);
                    });
                }
            }
        }
        info!("DNS resolution task finished.");
    }

    /// Performs the actual resolution with retry logic for a single domain.
    #[instrument(skip_all, fields(domain = %domain))]
    async fn perform_resolution(
        domain: &str,
        resolver: Arc<dyn DnsResolver>,
        config: DnsRetryConfig,
        metrics: Arc<Metrics>,
    ) -> Result<DnsInfo, DnsError> {
        let retries = config.retries.unwrap_or(3);
        let backoff_ms = config.backoff_ms.unwrap_or(500);
        let mut last_error = None;

        for attempt in 0..=retries {
            match resolver.resolve(domain).await {
                Ok(dns_info) => {
                    return Ok(dns_info);
                }
                Err(DnsError::Resolution(e)) => {
                    // NXDOMAIN is a definitive failure, so we don't retry.
                    if is_nxdomain_error_str(&e) {
                        debug!(error = %e, "NXDOMAIN detected, not retrying.");
                        // The error is returned to the caller, but since we don't use the
                        // result in the spawning task, this effectively ends the flow for this domain.
                        return Err(DnsError::Resolution(e));
                    }

                    metrics::counter!("dns_queries_total", "status" => "failure").increment(1);
                    debug!(error = %e, "Handling resolution error");
                    last_error = Some(e);

                    if attempt < retries {
                        let backoff_duration = backoff_ms * 2_u64.pow(attempt);
                        debug!(backoff_ms = backoff_duration, "Retrying after backoff");
                        let sleep_start = Instant::now();
                        sleep(Duration::from_millis(backoff_duration)).await;
                        metrics
                            .dns_retry_backoff_delay_seconds
                            .record(sleep_start.elapsed());
                    }
                }
                Err(DnsError::Shutdown) => {
                    warn!("Resolver unexpectedly returned a shutdown error");
                    return Err(DnsError::Shutdown);
                }
            }
        }

        let final_error = DnsError::Resolution(
            last_error.unwrap_or_else(|| format!("DNS resolution failed after {} retries", retries)),
        );
        debug!(error = %final_error, "Giving up on DNS resolution for domain");
        Err(final_error)
    }
}