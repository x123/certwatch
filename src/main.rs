//! CertWatch - Certificate Transparency Log Monitor
//!
//! A high-performance Rust application for monitoring certificate transparency
//! logs and detecting suspicious domain registrations in real-time.

use anyhow::Result;
use certwatch::{
    build_alert,
    cli::Cli,
    config::{Config, OutputFormat},
    core::{Alert, DnsInfo, EnrichmentProvider, Output, PatternMatcher},
    deduplication::Deduplicator,
    dns::{DnsHealthMonitor, DnsResolutionManager, HickoryDnsResolver},
    enrichment::tsv_lookup::TsvAsnLookup,
    matching::PatternWatcher,
    metrics::logging_recorder::LoggingRecorder,
    network::CertStreamClient,
    outputs::{OutputManager, SlackOutput, StdoutOutput},
};
use certwatch::utils::heartbeat::run_heartbeat;
use clap::Parser;
use log::{error, info, warn};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, Mutex, watch},
    task::JoinHandle,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration by layering sources: defaults, file, environment, and CLI args.
    let config = Config::load(&cli).unwrap_or_else(|err| {
        // Manually initialize logger for this specific error
        env_logger::init();
        error!("Failed to load configuration: {}", err);
        // Exit if configuration fails, as it's a critical step.
        std::process::exit(1);
    });

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
        .init();

    info!("CertWatch starting up...");

    // Log the loaded configuration settings for visibility
    info!("-------------------- Configuration --------------------");
    info!("Log Level: {}", config.log_level);
    info!("Log Metrics: {}", config.metrics.log_metrics);
    info!(
        "Log Aggregation Interval: {}s",
        config.metrics.log_aggregation_seconds
    );
    info!("Concurrency: {}", config.concurrency);
    info!(
        "Domain Queue Capacity: {}",
        config.performance.queue_capacity
    );
    info!("CertStream URL: {}", config.network.certstream_url);
    info!("Sample Rate: {}", config.network.sample_rate);
    let (dns_resolver, nameservers) = HickoryDnsResolver::from_config(&config.dns)?;
    if let Some(resolver) = &config.dns.resolver {
        info!("DNS Resolver: {}", resolver);
    } else {
        let ns_str: Vec<String> = nameservers.iter().map(|s| s.to_string()).collect();
        info!("DNS Resolver: System Default ({})", ns_str.join(", "));
    }
    if let Some(timeout) = config.dns.timeout_ms {
        info!("DNS Timeout: {}ms", timeout);
    }
    info!(
        "DNS Standard Retries: {}",
        config.dns.retry_config.standard_retries
    );
    info!(
        "DNS Standard Backoff: {}ms",
        config.dns.retry_config.standard_initial_backoff_ms
    );
    info!(
        "DNS NXDOMAIN Retries: {}",
        config.dns.retry_config.nxdomain_retries
    );
    info!(
        "DNS NXDOMAIN Backoff: {}ms",
        config.dns.retry_config.nxdomain_initial_backoff_ms
    );
    if let Some(path) = &config.enrichment.asn_tsv_path {
        info!("ASN TSV Path: {}", path.display());
    } else {
        info!("ASN TSV Path: Not configured");
    }
    let output_format = if cli.json {
        OutputFormat::Json
    } else {
        config.output.format.clone()
    };
    info!("Output Format: {}", output_format);
    info!(
        "Slack Output: {}",
        if config.output.slack.is_some() {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    info!(
        "Deduplication Cache Size: {}",
        config.deduplication.cache_size
    );
    info!(
        "Deduplication Cache TTL: {}s",
        config.deduplication.cache_ttl_seconds
    );
    info!("-------------------------------------------------------");

    // =========================================================================
    // Create Shutdown Channel
    // =========================================================================
    let (shutdown_tx, shutdown_rx) = watch::channel(());

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
    let dns_resolver = Arc::new(dns_resolver);
    // The enrichment provider is now determined by the presence of the tsv_path.
    // If the path is not provided, the program will fail to start.
    // A future improvement could be to use a "null" provider.
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
    let mut outputs: Vec<Box<dyn Output>> = Vec::new();
    outputs.push(Box::new(StdoutOutput::new(output_format.clone())));
    if let Some(slack_config) = &config.output.slack {
        outputs.push(Box::new(SlackOutput::new(
            slack_config.webhook_url.clone(),
        )));
    }
    // For file-based JSON output
    // outputs.push(Box::new(JsonOutput::new("alerts.log")?));
    let output_manager = Arc::new(OutputManager::new(outputs));

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
            if let Err(e) = certstream_client.run(shutdown_rx_clone).await {
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
            info!("Worker {} started", i);
            loop {
                // First, wait for a domain or a shutdown signal. The `async` block
                // here contains the lifetime of the MutexGuard, fixing the borrow
                // checker error.
                let domain_to_process = tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        info!("Worker {} received shutdown signal, exiting.", i);
                        break;
                    }
                    domain_option = async { domains_rx.lock().await.recv().await } => {
                        domain_option
                    }
                };

                let domain = match domain_to_process {
                    Some(domain) => domain,
                    None => {
                        info!("Domain channel closed, worker {} shutting down.", i);
                        break;
                    }
                };

                // Now, process the domain, but allow the processing to be
                // interrupted by a shutdown signal.
                let process_fut = async {
                    if let Some(source_tag) = pattern_matcher.match_domain(&domain).await {
                        info!("Worker {} matched domain: {} (source: {})", i, domain, source_tag);
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
                                if let Err(e) = alerts_tx.send(alert).await {
                                    error!("Worker {} failed to send alert to channel: {}", i, e);
                                }
                            }
                            Err(e) => {
                                if e.to_string().contains("NXDOMAIN") {
                                    let alert = build_alert(
                                        domain,
                                        source_tag.to_string(),
                                        false,
                                        DnsInfo::default(),
                                        enrichment_provider.clone(),
                                    )
                                    .await;
                                    if let Err(e) = alerts_tx.send(alert).await {
                                        error!(
                                            "Worker {} failed to send NXDOMAIN alert to channel: {}",
                                            i, e
                                        );
                                    }
                                } else {
                                    warn!("Worker {} failed DNS resolution for {}: {}", i, domain, e);
                                }
                            }
                        }
                    }
                };

                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        info!("Worker {} received shutdown signal during processing, aborting domain.", i);
                        // The loop will terminate on the next iteration.
                    }
                    _ = process_fut => {
                        // Processing completed normally.
                    }
                }
            }
        });
        worker_handles.push(handle);
    }

    // Drop the main thread's reference to the DNS manager. This allows the
    // NXDOMAIN feedback loop to terminate when all workers are done and drop
    // their references to the manager.

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
            // Spawn a heartbeat for the output manager
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
                        // Channel is empty and closed
                        break;
                    }
                }
            }
            info!("Output task finished.");
        })
    };

    info!("CertWatch initialized successfully. Monitoring for domains...");

    // Wait for shutdown signal
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Shutting down gracefully...");

    // Send shutdown signal to all tasks
    shutdown_tx.send(()).expect("Failed to send shutdown signal");

    // Wait for all tasks to complete
    if let Err(e) = certstream_task.await {
        error!("CertStream task panicked: {:?}", e);
    }

    // Wait for all worker tasks to complete. They will exit gracefully when the
    // domain channel is closed by the CertStream client shutting down.
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

    info!("All tasks shut down. Exiting.");

    Ok(())
}
