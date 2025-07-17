use crate::{
    config::PerformanceConfig,
    core::{DnsInfo, DnsResolver},
    dns::{DnsError, DnsHealth, DnsRetryConfig},
    internal_metrics::Metrics,
    task_manager::TaskManager,
};
use std::{sync::Arc, time::Duration};
use tokio::{
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

/// Manages DNS resolution tasks by distributing requests to a pool of workers.
pub struct DnsResolutionManager {
    dns_request_tx: async_channel::Sender<(String, String, Instant)>,
    health_monitor: Arc<DnsHealth>,
}

impl DnsResolutionManager {
    /// Creates a new DNS resolution manager and starts its background processing task.
    pub fn start(
        resolver: Arc<dyn DnsResolver>,
        retry_config: DnsRetryConfig,
        performance_config: &PerformanceConfig,
        health_monitor: Arc<DnsHealth>,
        task_manager: &TaskManager,
        metrics: Arc<Metrics>,
    ) -> (Arc<Self>, async_channel::Receiver<ResolvedDomain>) {
        let (manager_tx, manager_rx): (
            async_channel::Sender<(String, String, Instant)>,
            async_channel::Receiver<(String, String, Instant)>,
        ) = async_channel::bounded(performance_config.queue_capacity);
        let (resolved_tx, resolved_rx): (
            async_channel::Sender<ResolvedDomain>,
            async_channel::Receiver<ResolvedDomain>,
        ) = async_channel::bounded(performance_config.queue_capacity);

        let mut worker_txs = Vec::new();
        for i in 0..performance_config.dns_worker_concurrency {
            let (worker_tx, worker_rx): (
                async_channel::Sender<(String, String, Instant)>,
                async_channel::Receiver<(String, String, Instant)>,
            ) = async_channel::bounded(performance_config.queue_capacity);
            worker_txs.push(worker_tx);

            let mut worker_shutdown_rx = task_manager.get_shutdown_rx();
            let worker_resolver = resolver.clone();
            let worker_retry_config = retry_config.clone();
            let worker_resolved_tx = resolved_tx.clone();
            let worker_metrics = metrics.clone();

            task_manager.spawn("DnsWorker", async move {
                info!("DNS worker {} started.", i);
                loop {
                    tokio::select! {
                        biased;
                        _ = worker_shutdown_rx.changed() => {
                            info!("DNS worker {} received shutdown signal.", i);
                            break;
                        }
                        Ok((domain, _source_tag, start_time)) = worker_rx.recv() => {
                            match Self::perform_resolution(
                                &domain,
                                worker_resolver.clone(),
                                worker_retry_config.clone(),
                                worker_metrics.clone(),
                            )
                            .await
                            {
                                Ok(dns_info) => {
                                    if worker_resolved_tx
                                        .send((domain, dns_info, start_time))
                                        .await
                                        .is_err()
                                    {
                                        error!("Failed to send resolved domain to the next stage. Channel closed.");
                                    }
                                }
                                Err(e) => {
                                    error!(domain = %domain, error = %e, "DNS resolution failed for domain.");
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

        // This is the central distribution task
        let mut manager_shutdown_rx = task_manager.get_shutdown_rx();
        task_manager.spawn("DnsManager", async move {
            let mut next_worker = 0;
            info!("DNS Manager distributor task started.");
            loop {
                tokio::select! {
                    biased;
                    _ = manager_shutdown_rx.changed() => {
                        info!("DNS Manager distributor task received shutdown signal.");
                        break;
                    }
                    Ok(request) = manager_rx.recv() => {
                        if let Err(e) = worker_txs[next_worker].send(request).await {
                            error!("Failed to send domain to worker {}: {}", next_worker, e);
                        }
                        next_worker = (next_worker + 1) % worker_txs.len();
                    }
                    else => {
                        info!("Manager channel closed. Shutting down distributor.");
                        break;
                    }
                }
            }
        });


        let manager = Arc::new(Self {
            dns_request_tx: manager_tx,
            health_monitor,
        });

        (manager, resolved_rx)
    }

    /// Sends a domain to the resolution channel. This is a non-blocking call.
    pub async fn resolve(
        &self,
        domain: String,
        source_tag: String,
        start_time: Instant,
    ) -> Result<(), DnsError> {
        if !self.health_monitor.is_healthy() {
            metrics::counter!("dns_queries_total", "status" => "skipped_unhealthy").increment(1);
            warn!("DNS resolvers are unhealthy, skipping resolution for {}", domain);
            // Silently drop the domain. The error is logged by the health monitor.
            return Ok(());
        }

        self.dns_request_tx
            .send((domain, source_tag, start_time))
            .await
            .map_err(|e| {
                error!("DNS request channel is closed: {}", e);
                DnsError::Shutdown
            })
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