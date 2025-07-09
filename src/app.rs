//! The main application logic, decoupled from the entry point.

use crate::{
    build_alert,
    config::Config,
    core::{Alert, DnsInfo, DnsResolver, EnrichmentProvider, Output, PatternMatcher},
    deduplication::Deduplicator,
    dns::{DnsError, DnsHealthMonitor, DnsResolutionManager, HickoryDnsResolver},
    internal_metrics::logging_recorder::LoggingRecorder,
    matching::PatternWatcher,
    network::{CertStreamClient, WebSocketConnection},
    outputs::{OutputManager, StdoutOutput},
    rules::{EnrichmentLevel, RuleMatcher},
    types::AlertSender,
    utils::heartbeat::run_heartbeat,
};
use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tracing::{debug, error, info, instrument};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::task::JoinHandle;


/// Runs the main application logic.
#[instrument(skip_all)]
pub async fn run(
    config: Config,
    mut shutdown_rx: watch::Receiver<()>,
    domains_rx_override: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    output_override: Option<Vec<Arc<dyn Output>>>,
    websocket_override: Option<Box<dyn WebSocketConnection>>,
    dns_resolver_override: Option<Arc<dyn DnsResolver>>,
    enrichment_provider_override: Option<Arc<dyn EnrichmentProvider>>,
    pattern_matcher_override: Option<Arc<dyn PatternMatcher>>,
    _alert_tx: Option<broadcast::Sender<Alert>>,
) -> Result<()> {
    // =========================================================================
    // 1. Initialize Metrics Recorder (and logging)
    //
    // This MUST happen first so that startup logs are visible.
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
    // 2. Pre-flight Checks
    // =========================================================================
    // =========================================================================
    // 2. Pre-flight Checks & Service Instantiation
    // =========================================================================
    let dns_resolver = match dns_resolver_override {
        Some(resolver) => resolver,
        None => {
            let (resolver, _nameservers) = HickoryDnsResolver::from_config(&config.dns)?;
            Arc::new(resolver)
        }
    };

    DnsHealthMonitor::startup_check(dns_resolver.as_ref(), &config.dns.health).await?;

    // =========================================================================
    // 3. Instantiate Remaining Services
    // =========================================================================
    let pattern_matcher = match pattern_matcher_override {
        Some(matcher) => matcher,
        None => Arc::new(
            PatternWatcher::new(config.matching.pattern_files.clone(), shutdown_rx.clone()).await?,
        ),
    };
    let rule_matcher = Arc::new(RuleMatcher::load(&config.rules)?);

    let enrichment_provider = enrichment_provider_override
        .expect("Enrichment provider is now required to be passed into app::run");
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
            let outputs: Vec<Arc<dyn Output>> =
                vec![Arc::new(StdoutOutput::new(config.output.format.clone()))];
            Arc::new(OutputManager::new(outputs))
        }
    };

    // =========================================================================
    // 3. Create Channels for the Pipeline
    // =========================================================================
    let (alerts_tx, _alerts_rx) = broadcast::channel::<Alert>(1000);
    let (domains_tx, domains_rx) = if let Some(rx_override) = domains_rx_override {
        // This path is for testing. The test harness provides the receiver.
        // We still need a sender to give to the CertStreamClient, but it will be
        // disconnected and unused in the test environment.
        let (tx, _) = mpsc::channel::<String>(1000);
        (tx, rx_override)
    } else {
        // This is the production path.
        let (tx, rx) = mpsc::channel::<String>(1000);
        (tx, Arc::new(Mutex::new(rx)))
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
    // 6. Main Processing Loop (Worker Pool)
    // =========================================================================
    let mut worker_handles = Vec::new();
    info!("Spawning {} worker tasks...", config.concurrency);

    for i in 0..config.concurrency {
        let domains_rx = domains_rx.clone();
        let pattern_matcher = pattern_matcher.clone();
        let rule_matcher = rule_matcher.clone();
        let dns_manager = dns_manager.clone();
        let enrichment_provider = enrichment_provider.clone();
        let alerts_tx = alerts_tx.clone();
        let mut shutdown_rx = shutdown_rx.clone();

        let handle = tokio::spawn(async move {
            debug!("Worker {} started", i);
            loop {
                // Acquire the lock before the select to ensure the MutexGuard's lifetime is long enough.
                let mut guard = domains_rx.lock().await;

                let domain_to_process = tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        // Drop the guard to release the lock before exiting the loop.
                        drop(guard);
                        debug!("Worker {} received shutdown signal, exiting.", i);
                        break;
                    }
                    domain_opt = guard.recv() => {
                        domain_opt
                    }
                };

                let domain = match domain_to_process {
                    Some(domain) => domain,
                    None => {
                        // The channel is closed, which means the sender (CertStream client) has shut down.
                        debug!("Domain channel closed, worker {} shutting down.", i);
                        break;
                    }
                };

                debug!(worker_id = i, domain = %domain, "Worker processing domain");

                if rule_matcher.is_ignored(&domain) {
                    continue;
                }

                let process_fut = process_domain(
                    domain.clone(),
                    i,
                    pattern_matcher.clone(),
                    rule_matcher.clone(),
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
                                debug!("Failed to process certificate update for domain {}: {}", domain, e);
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
                match build_alert(
                    domain,
                    source_tag,
                    true,
                    dns_info,
                    enrichment_provider.clone(),
                )
                .await
                {
                    Ok(alert) => {
                        if let Err(e) = alerts_tx.send(alert) {
                            error!("Failed to send resolved NXDOMAIN alert to channel: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to build alert for resolved NXDOMAIN: {}", e);
                    }
                }
            }
            info!("NXDOMAIN feedback loop finished.");
        })
    };

    // =========================================================================
    // 8. Alert Deduplication and Output Task
    // =========================================================================
    let output_task = tokio::spawn(output_task_logic(
        shutdown_rx.clone(),
        alerts_tx.subscribe(),
        deduplicator,
        output_manager,
        Some(alerts_tx),
    ));

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

#[instrument(skip_all)]
async fn output_task_logic(
    mut shutdown_rx: watch::Receiver<()>,
    mut alerts_rx: broadcast::Receiver<Alert>,
    deduplicator: Arc<Deduplicator>,
    output_manager: Arc<OutputManager>,
    alert_tx: Option<broadcast::Sender<Alert>>,
) {
    let hb_shutdown_rx = shutdown_rx.clone();
    tokio::spawn(async move { run_heartbeat("OutputManager", hb_shutdown_rx).await });

    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                info!("Output task received shutdown signal.");
                break;
            }
            Ok(alert) = alerts_rx.recv() => {
                debug!("Output task received alert for domain: {}", &alert.domain);
                if !deduplicator.is_duplicate(&alert).await {
                    debug!("Domain {} is not a duplicate. Processing.", &alert.domain);
                    metrics::counter!("agg.alerts_sent").increment(1);

                    // Publish to notification pipeline if enabled
                    if let Some(tx) = &alert_tx {
                        debug!("Publishing alert to notification channel for domain: {}", &alert.domain);
                        if let Err(e) = tx.send(alert.clone()) {
                            error!(domain = %alert.domain, error = %e, "Failed to publish alert to notification channel");
                        }
                    }

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
}


/// Processes a single domain: matches, resolves, enriches, and sends alerts.
#[instrument(skip_all, fields(domain = %domain, worker_id = worker_id))]
pub async fn process_domain(
    domain: String,
    worker_id: usize,
    pattern_matcher: Arc<dyn PatternMatcher>,
    rule_matcher: Arc<RuleMatcher>,
    dns_manager: Arc<DnsResolutionManager>,
    enrichment_provider: Arc<dyn EnrichmentProvider>,
    alerts_tx: AlertSender,
) -> Result<()> {
    // STAGE 1: Pre-enrichment filtering (legacy patterns and Stage 1 rules)
    let legacy_match = pattern_matcher.match_domain(&domain).await;
    let stage_1_rule_matches = {
        let _span = tracing::info_span!("stage_1_rule_matching").entered();
        let base_alert = Alert::new_minimal(&domain);
        rule_matcher.matches(&base_alert, EnrichmentLevel::None)
    };

    // If there's no match from either legacy patterns or Stage 1 rules, we can stop.
    if legacy_match.is_none() && stage_1_rule_matches.is_empty() {
        debug!("Domain did not match any pre-enrichment patterns or rules.");
        return Ok(());
    }
    debug!("Domain matched pre-enrichment checks. Proceeding to enrichment.");

    // STAGE 2: Enrichment and Post-enrichment filtering
    let result = dns_manager
        .resolve_with_retry(&domain, "enrichment_candidate")
        .await;
    debug!(?result, "DNS resolution result");

    let dns_info = match result {
        Ok(ref info) => info.clone(),
        Err(DnsError::Resolution(ref e)) if e.to_string().contains("NXDOMAIN") => {
            DnsInfo::default()
        }
        Err(DnsError::Resolution(e)) => {
            debug!(error = %e, "DNS resolution failed");
            anyhow::bail!("DNS resolution failed: {}", e);
        }
        Err(DnsError::Shutdown) => {
            debug!("DNS resolution cancelled by shutdown.");
            return Ok(());
        }
    };

    let resolved_after_nxdomain = result.is_ok() && dns_info.is_empty();
    let mut alert = build_alert(
        domain,
        "placeholder".to_string(), // Source will be replaced with combined rule names
        resolved_after_nxdomain,
        dns_info,
        enrichment_provider.clone(),
    )
    .await
    .context("Failed to build alert")?;

    let stage_2_rule_matches = {
        let _span = tracing::info_span!("stage_2_rule_matching").entered();
        rule_matcher.matches(&alert, EnrichmentLevel::Standard)
    };

    // FINAL DECISION: Combine all matches to determine if we should alert.
    let mut all_matches_set: HashSet<String> = stage_1_rule_matches.into_iter().collect();
    all_matches_set.extend(stage_2_rule_matches);
    if let Some(tag) = legacy_match {
        all_matches_set.insert(tag);
    }

    if all_matches_set.is_empty() {
        // This can happen if a domain matched a legacy pattern, but the rules engine
        // logic (which is now the source of truth) doesn't have a corresponding rule.
        // Or if a Stage 1 rule was a false positive that was correctly filtered by Stage 2.
        debug!("Domain was enriched but did not match any final rules. Discarding.");
        return Ok(());
    }

    // Sort for consistent output order before joining
    let mut all_matches: Vec<String> = all_matches_set.into_iter().collect();
    all_matches.sort();

    // Update the alert with the final, combined source tags.
    alert.source_tag = all_matches.join(", ");

    debug!(source = %alert.source_tag, "Domain passed all checks. Sending alert.");
    if alerts_tx.send(alert).is_err() {
        anyhow::bail!("Alerts channel closed, worker {} exiting.", worker_id);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::RulesConfig,
        core::{DnsInfo, DnsResolver, PatternMatcher},
        dns::{DnsError, DnsResolutionManager},
        enrichment::fake::FakeEnrichmentProvider,
        rules::RuleMatcher,
    };
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch};

    // A mock pattern matcher for testing purposes.
    struct MockPatternMatcher {
        match_domain: bool,
    }

    #[async_trait::async_trait]
    impl PatternMatcher for MockPatternMatcher {
        async fn match_domain(&self, _domain: &str) -> Option<String> {
            if self.match_domain {
                Some("test-source".to_string())
            } else {
                None
            }
        }
    }

    // A mock DNS resolver for testing purposes.
    #[derive(Clone)]
    struct MockDnsResolver {
        resolve_to: Result<DnsInfo, DnsError>,
    }

    #[async_trait::async_trait]
    impl DnsResolver for MockDnsResolver {
        async fn resolve(&self, _domain: &str) -> Result<DnsInfo, DnsError> {
            self.resolve_to.clone()
        }
    }

    #[tokio::test]
    async fn test_process_domain_dns_timeout_propagates_error() {
        // Arrange
        let pattern_matcher = Arc::new(MockPatternMatcher { match_domain: true });
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (_resolver, _health_monitor, _dns_manager) = {
            let resolver = Arc::new(MockDnsResolver {
                resolve_to: Err(DnsError::Resolution("timeout".to_string())),
            });
            let health_monitor = DnsHealthMonitor::new(
                Default::default(),
                resolver.clone(),
                shutdown_rx.clone(),
            );
            let (manager, _) = DnsResolutionManager::new(
                resolver.clone(),
                Default::default(),
                health_monitor.clone(),
                shutdown_rx.clone(),
            );
            (resolver, health_monitor, manager)
        };

        let dns_manager = Arc::new(_dns_manager);
        let enrichment_provider = Arc::new(FakeEnrichmentProvider::new());
        let (alerts_tx, _alerts_rx) = broadcast::channel(1);

        // Act
        let result = process_domain(
            "timeout.com".to_string(),
            0,
            pattern_matcher,
            Arc::new(RuleMatcher::load(&RulesConfig { rule_files: vec![] }).unwrap()),
            dns_manager,
            enrichment_provider,
            alerts_tx.clone(),
        )
        .await;

        // Assert
        assert!(result.is_err(), "Expected an error due to DNS timeout");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("DNS resolution failed"),
            "Error message should indicate a DNS resolution failure"
        );

        // ensure shutdown channel is used
        drop(shutdown_tx);
    }
}
