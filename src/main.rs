//! CertWatch - Certificate Transparency Log Monitor
//!
//! A high-performance Rust application for monitoring certificate transparency
//! logs and detecting suspicious domain registrations in real-time.

use anyhow::Result;
use certwatch::{
    config::{AsnProvider, Config},
    core::{Alert, DnsInfo, EnrichmentProvider, Output, PatternMatcher},
    deduplication::Deduplicator,
    dns::{DnsResolutionManager, TrustDnsResolver},
    enrichment::{tsv_lookup::TsvAsnLookup, MaxmindEnrichmentProvider},
    matching::PatternWatcher,
    network::CertStreamClient,
    outputs::{OutputManager, SlackOutput, StdoutOutput},
};
use chrono::Utc;
use log::{error, info};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration first, so we can use it for logging
    // In a real app, this path would come from a CLI argument.
    let config = Config::load("certwatch.toml").unwrap_or_else(|err| {
        // Manually initialize logger for this error message
        env_logger::init();
        error!("Failed to load configuration: {}", err);
        // Fallback to default for basic operation if config file is missing
        Config::default()
    });

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
        .init();

    info!("CertWatch starting up...");

    // Load configuration
    // In a real app, this path would come from a CLI argument.

    // =========================================================================
    // 1. Instantiate Services
    // =========================================================================
    let pattern_matcher = Arc::new(PatternWatcher::new(config.matching.pattern_files.clone()).await?);
    let dns_resolver = Arc::new(TrustDnsResolver::new()?);
    let enrichment_provider: Arc<dyn EnrichmentProvider> =
        match config.enrichment.asn_provider {
            AsnProvider::Maxmind => {
                let asn_db_path = config.enrichment.asn_db_path.clone().ok_or_else(|| {
                    anyhow::anyhow!("asn_db_path is required for Maxmind provider")
                })?;
                Arc::new(MaxmindEnrichmentProvider::new(
                    &asn_db_path,
                    &config.enrichment.geoip_db_path,
                )?)
            }
            AsnProvider::Tsv => {
                let tsv_path = config.enrichment.asn_tsv_path.clone().ok_or_else(|| {
                    anyhow::anyhow!("asn_tsv_path is required for Tsv provider")
                })?;
                Arc::new(TsvAsnLookup::new(&tsv_path)?)
            }
        };
    let deduplicator = Arc::new(Deduplicator::new(
        Duration::from_secs(config.deduplication.cache_ttl_seconds),
        config.deduplication.cache_size as u64,
    ));

    // =========================================================================
    // 2. Setup Output Manager
    // =========================================================================
    let mut outputs: Vec<Box<dyn Output>> = Vec::new();
    // Always add stdout output, but its behavior is controlled by the format.
    outputs.push(Box::new(StdoutOutput::new(config.output.format.clone())));
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
    let (domains_tx, mut domains_rx) = mpsc::channel::<Vec<String>>(1000);
    let (alerts_tx, mut alerts_rx) = mpsc::channel::<Alert>(1000);

    // =========================================================================
    // 4. Start the CertStream Client
    // =========================================================================
    let certstream_client = CertStreamClient::new(
        config.network.certstream_url.clone(),
        domains_tx,
        config.network.sample_rate,
        config.network.allow_invalid_certs,
    );
    tokio::spawn(async move {
        if let Err(e) = certstream_client.run().await {
            error!("CertStream client failed: {}", e);
        }
    });

    // =========================================================================
    // 5. Start the DNS Resolution Manager
    // =========================================================================
    let (dns_manager, mut resolved_nxdomain_rx) = DnsResolutionManager::new(
        dns_resolver.clone(),
        config.dns.retry_config.clone(),
    );
    let dns_manager = Arc::new(dns_manager);

    // =========================================================================
    // 6. Main Processing Loop (The "Coordinator")
    // =========================================================================
    let main_processing_task = {
        let pattern_matcher = pattern_matcher.clone();
        let dns_manager = dns_manager.clone();
        let enrichment_provider = enrichment_provider.clone();
        let alerts_tx = alerts_tx.clone();

        tokio::spawn(async move {
            while let Some(domains) = domains_rx.recv().await {
                for domain in domains {
                    let pattern_matcher = pattern_matcher.clone();
                    let dns_manager = dns_manager.clone();
                    let enrichment_provider = enrichment_provider.clone();
                    let alerts_tx = alerts_tx.clone();

                    tokio::spawn(async move {
                        if let Some(source_tag) = pattern_matcher.match_domain(&domain).await {
                            info!("Matched domain: {} (source: {})", domain, source_tag);
                            match dns_manager.resolve_with_retry(&domain, &source_tag).await {
                                Ok(dns_info) => {
                                    let alert =
                                        build_alert(domain, source_tag.to_string(), false, dns_info, enrichment_provider).await;
                                    if let Err(e) = alerts_tx.send(alert).await {
                                        error!("Failed to send alert to channel: {}", e);
                                    }
                                }
                                Err(e) => {
                                    if e.to_string().contains("NXDOMAIN") {
                                        // Create an initial alert for the NXDOMAIN finding
                                        let alert = build_alert(domain, source_tag.to_string(), false, DnsInfo::default(), enrichment_provider).await;
                                         if let Err(e) = alerts_tx.send(alert).await {
                                            error!("Failed to send NXDOMAIN alert to channel: {}", e);
                                        }
                                    } else {
                                        error!("DNS resolution failed for {}: {}", domain, e);
                                    }
                                }
                            }
                        }
                    });
                }
            }
        })
    };

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
                let alert =
                    build_alert(domain, source_tag, true, dns_info, enrichment_provider.clone()).await;
                if let Err(e) = alerts_tx.send(alert).await {
                    error!("Failed to send resolved NXDOMAIN alert to channel: {}", e);
                }
            }
        })
    };

    // =========================================================================
    // 8. Alert Deduplication and Output Task
    // =========================================================================
    let output_task = tokio::spawn(async move {
        while let Some(alert) = alerts_rx.recv().await {
            if !deduplicator.is_duplicate(&alert).await {
                info!("Sending alert for domain: {}", alert.domain);
                if let Err(e) = output_manager.send_alert(&alert).await {
                    error!("Failed to send alert via output manager: {}", e);
                }
            }
        }
    });

    info!("CertWatch initialized successfully. Monitoring for domains...");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down CertWatch...");

    // Gracefully shutdown tasks
    main_processing_task.abort();
    nxdomain_feedback_task.abort();
    output_task.abort();

    Ok(())
}

/// Helper function to build an alert
async fn build_alert(
    domain: String,
    source_tag: String,
    resolved_after_nxdomain: bool,
    dns_info: DnsInfo,
    enrichment_provider: Arc<dyn EnrichmentProvider>,
) -> Alert {
    let mut enrichment_data = Vec::new();
    let all_ips: Vec<_> = dns_info
        .a_records
        .iter()
        .chain(dns_info.aaaa_records.iter())
        .cloned()
        .collect();

    for ip in all_ips {
        match enrichment_provider.enrich(ip).await {
            Ok(info) => enrichment_data.push(info),
            Err(e) => error!("Failed to enrich IP {}: {}", ip, e),
        }
    }

    Alert {
        timestamp: Utc::now().to_rfc3339(),
        domain,
        source_tag,
        resolved_after_nxdomain,
        dns: dns_info,
        enrichment: enrichment_data,
    }
}
