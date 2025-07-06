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
use clap::Parser;
use log::{error, info, warn};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, Mutex, watch};

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
    // Initialize Metrics Recorder if enabled
    // =========================================================================
    if config.metrics.log_metrics {
        info!(
            "Logging recorder enabled. Metrics will be printed every {} seconds.",
            config.metrics.log_aggregation_seconds
        );
        let recorder = LoggingRecorder::new(Duration::from_secs(
            config.metrics.log_aggregation_seconds,
        ));
        metrics::set_global_recorder(recorder).expect("Failed to install logging recorder");
    }

    // =========================================================================
    // 1. Instantiate Services
    // =========================================================================
    let pattern_matcher = Arc::new(PatternWatcher::new(config.matching.pattern_files.clone()).await?);
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
    let (shutdown_tx, shutdown_rx) = watch::channel(());

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
        let mut shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            tokio::select! {
                res = certstream_client.run() => {
                    if let Err(e) = res {
                        error!("CertStream client failed: {}", e);
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("CertStream client received shutdown signal.");
                }
            }
        })
    };

    // =========================================================================
    // 5. Start the DNS Resolution Manager
    // =========================================================================
    let dns_health_monitor =
        DnsHealthMonitor::new(config.dns.health.clone(), dns_resolver.clone());
    let (dns_manager, mut resolved_nxdomain_rx) = DnsResolutionManager::new(
        dns_resolver.clone(),
        config.dns.retry_config.clone(),
        dns_health_monitor,
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

        let handle = tokio::spawn(async move {
            info!("Worker {} started", i);
            loop {
                // Lock the mutex to receive a domain from the shared receiver
                let domain = match domains_rx.lock().await.recv().await {
                    Some(domain) => domain,
                    None => {
                        // Channel is empty and closed, so the worker can exit.
                        info!("Domain channel closed, worker {} shutting down.", i);
                        break;
                    }
                };

                // The core processing logic for a single domain
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
        let mut shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some((domain, source_tag, dns_info)) = resolved_nxdomain_rx.recv() => {
                        info!(
                            "Domain previously NXDOMAIN now resolves: {} ({})",
                            domain, source_tag
                        );
                        let alert = build_alert(domain, source_tag, true, dns_info, enrichment_provider.clone()).await;
                        if let Err(e) = alerts_tx.send(alert).await {
                            error!("Failed to send resolved NXDOMAIN alert to channel: {}", e);
                        }
                    },
                    _ = shutdown_rx.changed() => {
                        info!("NXDOMAIN feedback loop received shutdown signal.");
                        break;
                    }
                }
            }
        })
    };

    // =========================================================================
    // 8. Alert Deduplication and Output Task
    // =========================================================================
    let output_task = {
        let mut shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(alert) = alerts_rx.recv() => {
                        if !deduplicator.is_duplicate(&alert).await {
                            metrics::counter!("agg.alerts_sent").increment(1);
                            if let Err(e) = output_manager.send_alert(&alert).await {
                                error!("Failed to send alert via output manager: {}", e);
                            }
                        }
                    },
                    _ = shutdown_rx.changed() => {
                        info!("Output task received shutdown signal.");
                        break;
                    }
                }
            }
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

    if let Err(e) = output_task.await {
        error!("Output task panicked: {:?}", e);
    }

    info!("All tasks shut down. Exiting.");

    Ok(())
}
