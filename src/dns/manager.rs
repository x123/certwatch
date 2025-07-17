use crate::{
    config::PerformanceConfig,
    core::{DnsInfo, DnsResolver},
    dns::{DnsError, DnsHealth, DnsRetryConfig},
    internal_metrics::Metrics,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::watch,
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
    dns_request_tx: async_channel::Sender<(String, String, Instant)>,
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
        let (dns_request_tx, dns_request_rx) =
            async_channel::bounded::<(String, String, Instant)>(performance_config.queue_capacity);
        let (resolved_tx, resolved_rx) =
            async_channel::bounded(performance_config.queue_capacity);

        // Spawn a pool of worker tasks
        for i in 0..performance_config.dns_worker_concurrency {
            let mut worker_shutdown_rx = shutdown_rx.clone();
            let worker_resolver = resolver.clone();
            let worker_retry_config = retry_config.clone();
            let worker_dns_request_rx = dns_request_rx.clone();
            let worker_resolved_tx = resolved_tx.clone();
            let worker_metrics = metrics.clone();

            tokio::spawn(async move {
                info!("DNS worker {} started.", i);
                loop {
                    tokio::select! {
                        biased;
                        _ = worker_shutdown_rx.changed() => {
                            info!("DNS worker {} received shutdown signal.", i);
                            break;
                        }
                        Ok((domain, _source_tag, start_time)) = worker_dns_request_rx.recv() => {
                            if let Ok(dns_info) = Self::perform_resolution(
                                &domain,
                                worker_resolver.clone(),
                                worker_retry_config.clone(),
                                worker_metrics.clone(),
                            )
                            .await
                            {
                                if worker_resolved_tx
                                    .send((domain, dns_info, start_time))
                                    .await
                                    .is_err()
                                {
                                    error!("Failed to send resolved domain to the next stage. Channel closed.");
                                }
                            }
                        }
                        else => {
                            info!("DNS request channel closed. Shutting down worker {}.", i);
                            break;
                        }
                    }
                }
                info!("DNS worker {} finished.", i);
            });
        }

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
        match self
            .dns_request_tx
            .try_send((domain, source_tag, start_time))
        {
            Ok(_) => Ok(()),
            Err(async_channel::TrySendError::Full(_)) => {
                warn!("DNS request channel is full. Dropping domain.");
                Err(DnsError::Resolution("DNS channel full".to_string()))
            }
            Err(async_channel::TrySendError::Closed(_)) => {
                error!("DNS request channel is closed.");
                Err(DnsError::Shutdown)
            }
        }
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