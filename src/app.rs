//! The main application logic, decoupled from the entry point.

use crate::{
    build_alert,
    config::Config,
    core::{Alert, DnsInfo, DnsResolver, EnrichmentProvider, Output, PatternMatcher},
    deduplication::Deduplicator,
    dns::{DnsError, DnsHealthMonitor, DnsResolutionManager, HickoryDnsResolver},
    enrichment::tsv_lookup::TsvAsnLookup,
    matching::PatternWatcher,
    internal_metrics::logging_recorder::LoggingRecorder,
    network::{CertStreamClient, WebSocketConnection},
    outputs::{OutputManager, SlackOutput, StdoutOutput},
    utils::heartbeat::run_heartbeat,
};
use anyhow::Result;
use log::{debug, error, info, warn};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, Mutex, watch},
    task::JoinHandle,
};

/// Runs the main application logic.
pub async fn run(
    config: Config,
    mut shutdown_rx: watch::Receiver<()>,
    output_override: Option<Vec<Arc<dyn Output>>>,
    websocket_override: Option<Box<dyn WebSocketConnection>>,
    dns_resolver_override: Option<Arc<dyn DnsResolver>>,
) -> Result<()> {
    // =========================================================================
    // Initialize Metrics Recorder if enabled
    // =========================================================================
    let mut metrics_task: Option<JoinHandle<()>> = None;
    if config.metrics.log_metrics {
        info!(
            "Logging recorder enabled. Metrics will be printed every {} seconds.",
            config.metrics.log_aggregation_seconds
        );
        let (recorder, handle) = LoggingRecorder::new(
            Duration::from_secs(config.metrics.log_aggregation_seconds),
            shutdown_rx.clone(),
        );
        metrics::set_global_recorder(recorder).expect("Failed to install logging recorder");
        metrics_task = Some(handle);
    }

    // =========================================================================
    // 1. Instantiate Services
    // =========================================================================
    let pattern_matcher = Arc::new(
        PatternWatcher::new(config.matching.pattern_files.clone(), shutdown_rx.clone()).await?,
    );
    let dns_resolver = match dns_resolver_override {
        Some(resolver) => resolver,
        None => {
            let (resolver, _nameservers) = HickoryDnsResolver::from_config(&config.dns)?;
            Arc::new(resolver)
        }
    };
    let tsv_path = config.enrichment.asn_tsv_path.clone().ok_or_else(|| {
        anyhow::anyhow!("asn_tsv_path is required for enrichment")
    })?;

    if !tsv_path.exists() {
        return Err(anyhow::anyhow!("TSV database not found at {:?}", tsv_path));
    }

    let enrichment_provider: Arc<dyn EnrichmentProvider> = Arc::new(TsvAsnLookup::new(&tsv_path)?);
    let deduplicator = Arc::new(Deduplicator::new(
        Duration::from_secs(config.deduplication.cache_ttl_seconds),
        config.deduplication.cache_size as u64,
    ));

    // =========================================================================
    // 2. Setup Output Manager
    // =========================================================================
    let output_manager = match output_override {
        Some(outputs) => Arc::new(OutputManager::new(outputs)),
        None => {
            let mut outputs: Vec<Arc<dyn Output>> = Vec::new();
            outputs.push(Arc::new(StdoutOutput::new(config.output.format.clone())));
            if let Some(slack_config) = &config.output.slack {
                outputs.push(Arc::new(SlackOutput::new(
                    slack_config.webhook_url.clone(),
                )));
            }
            Arc::new(OutputManager::new(outputs))
        }
    };

    // =========================================================================
    // 3. Create Channels for the Pipeline
    // =========================================================================
    let (domains_tx, domains_rx) = mpsc::channel::<String>(config.performance.queue_capacity);
    let (alerts_tx, mut alerts_rx) = mpsc::channel::<Alert>(1000);

    // =========================================================================
    // 3.5 Start Domain Queue Monitoring Task
    // =========================================================================
    let queue_monitor_task = {
        let domains_tx = domains_tx.clone();
        let mut shutdown_rx = shutdown_rx.clone();
        let capacity = config.performance.queue_capacity as f64;

        tokio::spawn(async move {
            let hb_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                run_heartbeat("QueueMonitor", hb_shutdown_rx).await;
            });

            info!("Domain queue monitor started.");
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        info!("Queue monitor received shutdown signal.");
                        break;
                    }
                    _ = interval.tick() => {
                        let used = capacity - domains_tx.capacity() as f64;
                        let fill_ratio = if capacity > 0.0 { used / capacity } else { 0.0 };
                        metrics::gauge!("domain_queue_fill_ratio").set(fill_ratio);
                    }
                }
            }
            info!("Domain queue monitor finished.");
        })
    };

    // =========================================================================
    // 4. Start the CertStream Client
    // =========================================================================
    let certstream_client = CertStreamClient::new(
        config.network.certstream_url.clone(),
        domains_tx,
        config.network.sample_rate,
        config.network.allow_invalid_certs,
    );
    let certstream_task = {
        let shutdown_rx_clone = shutdown_rx.clone();
        tokio::spawn(async move {
            let result = if let Some(ws) = websocket_override {
                certstream_client.run_with_connection(ws).await
            } else {
                certstream_client.run(shutdown_rx_clone).await
            };

            if let Err(e) = result {
                error!("CertStream client failed: {}", e);
            }
        })
    };

    // =========================================================================
    // 5. Start the DNS Resolution Manager
    // =========================================================================
    let dns_health_monitor = DnsHealthMonitor::new(
        config.dns.health.clone(),
        dns_resolver.clone(),
        shutdown_rx.clone(),
    );
    let (dns_manager, mut resolved_nxdomain_rx) = DnsResolutionManager::new(
        dns_resolver.clone(),
        config.dns.retry_config.clone(),
        dns_health_monitor.clone(),
        shutdown_rx.clone(),
    );
    let dns_manager = Arc::new(dns_manager);

    // =========================================================================
    // 6. Main Processing Loop (Worker Pool)
    // =========================================================================
    let domains_rx = Arc::new(Mutex::new(domains_rx));
    let mut worker_handles = Vec::new();
    info!("Spawning {} worker tasks...", config.concurrency);

    for i in 0..config.concurrency {
        let domains_rx = domains_rx.clone();
        let pattern_matcher = pattern_matcher.clone();
        let dns_manager = dns_manager.clone();
        let enrichment_provider = enrichment_provider.clone();
        let alerts_tx = alerts_tx.clone();
        let mut shutdown_rx = shutdown_rx.clone();

        let handle = tokio::spawn(async move {
            debug!("Worker {} started", i);
            loop {
                let domain_to_process = tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        debug!("Worker {} received shutdown signal, exiting.", i);
                        break;
                    }
                    domain_option = async { domains_rx.lock().await.recv().await } => {
                        domain_option
                    }
                };

                let domain = match domain_to_process {
                    Some(domain) => domain,
                    None => {
                        debug!("Domain channel closed, worker {} shutting down.", i);
                        break;
                    }
                };

                let process_fut = process_domain(
                    domain.clone(),
                    i,
                    pattern_matcher.clone(),
                    dns_manager.clone(),
                    enrichment_provider.clone(),
                    alerts_tx.clone(),
                );

                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        debug!("Worker {} received shutdown signal during processing, aborting domain {}.", i, domain);
                    }
                    result = process_fut => {
                        match result {
                            Ok(_) => {
                                metrics::counter!("cert_processing_successes").increment(1);
                            }
                            Err(e) => {
                                metrics::counter!("cert_processing_failures").increment(1);
                                warn!("Failed to process certificate update for domain {}: {}", domain, e);
                            }
                        }
                    }
                }
            }
        });
        worker_handles.push(handle);
    }

    // =========================================================================
    // 7. NXDOMAIN Resolution Feedback Loop
    // =========================================================================
    let nxdomain_feedback_task = {
        let alerts_tx = alerts_tx.clone();
        let enrichment_provider = enrichment_provider.clone();
        tokio::spawn(async move {
            while let Some((domain, source_tag, dns_info)) = resolved_nxdomain_rx.recv().await {
                info!(
                    "Domain previously NXDOMAIN now resolves: {} ({})",
                    domain, source_tag
                );
                let alert = build_alert(
                    domain,
                    source_tag,
                    true,
                    dns_info,
                    enrichment_provider.clone(),
                )
                .await;
                if let Err(e) = alerts_tx.send(alert).await {
                    error!("Failed to send resolved NXDOMAIN alert to channel: {}", e);
                }
            }
            info!("NXDOMAIN feedback loop finished.");
        })
    };

    // =========================================================================
    // 8. Alert Deduplication and Output Task
    // =========================================================================
    let output_task = {
        let mut shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            let hb_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(
                async move { run_heartbeat("OutputManager", hb_shutdown_rx).await },
            );

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        info!("Output task received shutdown signal.");
                        break;
                    }
                    Some(alert) = alerts_rx.recv() => {
                        if !deduplicator.is_duplicate(&alert).await {
                            metrics::counter!("agg.alerts_sent").increment(1);
                            if let Err(e) = output_manager.send_alert(&alert).await {
                                error!("Failed to send alert via output manager: {}", e);
                            }
                        }
                    }
                    else => {
                        break;
                    }
                }
            }
            info!("Output task finished.");
        })
    };

    info!("CertWatch initialized successfully. Monitoring for domains...");

    // Wait for the external shutdown signal
    shutdown_rx.changed().await.ok();
    info!("Shutdown signal received in run function. Waiting for tasks to complete...");

    // Wait for all tasks to complete
    if let Err(e) = certstream_task.await {
        error!("CertStream task panicked: {:?}", e);
    }
    for handle in worker_handles {
        if let Err(e) = handle.await {
            error!("Worker task panicked: {:?}", e);
        }
    }
    if let Err(e) = nxdomain_feedback_task.await {
        error!("NXDOMAIN feedback task panicked: {:?}", e);
    }
    if let Err(e) = queue_monitor_task.await {
        error!("Queue monitor task panicked: {:?}", e);
    }
    if let Err(e) = output_task.await {
        error!("Output task panicked: {:?}", e);
    }
    if let Some(handle) = metrics_task {
        if let Err(e) = handle.await {
            error!("Metrics task panicked: {:?}", e);
        }
    }

    info!("All tasks shut down.");
    Ok(())
}

/// Processes a single domain: matches, resolves, enriches, and sends alerts.
async fn process_domain(
    domain: String,
    worker_id: usize,
    pattern_matcher: Arc<dyn PatternMatcher>,
    dns_manager: Arc<DnsResolutionManager>,
    enrichment_provider: Arc<dyn EnrichmentProvider>,
    alerts_tx: mpsc::Sender<Alert>,
) -> Result<()> {
    if let Some(source_tag) = pattern_matcher.match_domain(&domain).await {
        debug!(
            "Worker {} matched domain: {} (source: {})",
            worker_id, domain, source_tag
        );
        match dns_manager.resolve_with_retry(&domain, &source_tag).await {
            Ok(dns_info) => {
                let alert = build_alert(
                    domain,
                    source_tag.to_string(),
                    false,
                    dns_info,
                    enrichment_provider.clone(),
                )
                .await;
                if alerts_tx.send(alert).await.is_err() {
                    anyhow::bail!("Alerts channel closed, worker {} exiting.", worker_id);
                }
            }
            Err(DnsError::Resolution(e)) => {
                if e.to_string().contains("NXDOMAIN") {
                    let alert = build_alert(
                        domain,
                        source_tag.to_string(),
                        false,
                        DnsInfo::default(),
                        enrichment_provider.clone(),
                    )
                    .await;
                    if alerts_tx.send(alert).await.is_err() {
                        anyhow::bail!(
                            "Alerts channel (NXDOMAIN) closed, worker {} exiting.",
                            worker_id
                        );
                    }
                } else {
                    debug!(
                        "Worker {} failed DNS resolution for {}: {}",
                        worker_id, domain, e
                    );
                }
            }
            Err(DnsError::Shutdown) => {
                debug!(
                    "Worker {} DNS resolution cancelled by shutdown.",
                    worker_id
                );
            }
        }
    }
    Ok(())
}