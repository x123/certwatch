//! The main application logic, decoupled from the entry point.

use crate::{
    build_alert,
    config::Config,
    core::{Alert, DnsInfo, DnsResolver, EnrichmentProvider, Output},
    deduplication::{Deduplicator, InFlightDeduplicator},
    dns::{DnsError, DnsHealthMonitor, DnsResolutionManager, HickoryDnsResolver},
    internal_metrics::logging_recorder::LoggingRecorder,
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


/// A handle to the running application, containing all its task handles.
pub struct App {
    _config: Config,
    shutdown_rx: watch::Receiver<()>,
    certstream_task: JoinHandle<()>,
    worker_handles: Vec<JoinHandle<()>>,
    nxdomain_feedback_task: JoinHandle<()>,
    output_task: JoinHandle<()>,
    metrics_task: Option<JoinHandle<()>>,
}

impl App {
    /// Creates a new `AppBuilder` to construct an `App`.
    pub fn builder(config: Config) -> AppBuilder {
        AppBuilder::new(config)
    }

    /// Waits for the shutdown signal and then gracefully shuts down all tasks.
    pub async fn run(self) -> Result<()> {
        let mut shutdown_rx = self.shutdown_rx;
        // Wait for the external shutdown signal
        shutdown_rx.changed().await.ok();
        info!("Shutdown signal received in run function. Waiting for tasks to complete...");

        // Wait for all tasks to complete
        if let Err(e) = self.certstream_task.await {
            error!("CertStream task panicked: {:?}", e);
        }
        for handle in self.worker_handles {
            if let Err(e) = handle.await {
                error!("Worker task panicked: {:?}", e);
            }
        }
        if let Err(e) = self.nxdomain_feedback_task.await {
            error!("NXDOMAIN feedback task panicked: {:?}", e);
        }
        if let Err(e) = self.output_task.await {
            error!("Output task panicked: {:?}", e);
        }
        if let Some(handle) = self.metrics_task {
            if let Err(e) = handle.await {
                error!("Metrics task panicked: {:?}", e);
            }
        }

        info!("All tasks shut down.");
        Ok(())
    }
}

/// Builder for the main application.
///
/// This pattern allows for a clean separation of concerns between constructing
/// the application's components and running the application. It also provides
/// a convenient way to override components for testing purposes.
pub struct AppBuilder {
    config: Config,
    domains_rx_override: Option<Arc<Mutex<mpsc::Receiver<String>>>>,
    output_override: Option<Vec<Arc<dyn Output>>>,
    websocket_override: Option<Box<dyn WebSocketConnection>>,
    dns_resolver_override: Option<Arc<dyn DnsResolver>>,
    enrichment_provider_override: Option<Arc<dyn EnrichmentProvider>>,
    notification_tx: Option<broadcast::Sender<Alert>>,
}

impl AppBuilder {
    /// Creates a new `AppBuilder` with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            domains_rx_override: None,
            output_override: None,
            websocket_override: None,
            dns_resolver_override: None,
            enrichment_provider_override: None,
            notification_tx: None,
        }
    }

    /// Overrides the domain receiver channel for testing.
    pub fn domains_rx_override(mut self, rx: Arc<Mutex<mpsc::Receiver<String>>>) -> Self {
        self.domains_rx_override = Some(rx);
        self
    }

    /// Overrides the output channels for testing.
    pub fn output_override(mut self, outputs: Vec<Arc<dyn Output>>) -> Self {
        self.output_override = Some(outputs);
        self
    }

    /// Overrides the WebSocket connection for testing.
    pub fn websocket_override(mut self, ws: Box<dyn WebSocketConnection>) -> Self {
        self.websocket_override = Some(ws);
        self
    }

    /// Overrides the DNS resolver for testing.
    pub fn dns_resolver_override(mut self, resolver: Arc<dyn DnsResolver>) -> Self {
        self.dns_resolver_override = Some(resolver);
        self
    }

    /// Overrides the enrichment provider for testing.
    pub fn enrichment_provider_override(mut self, provider: Arc<dyn EnrichmentProvider>) -> Self {
        self.enrichment_provider_override = Some(provider);
        self
    }

    /// Overrides the notification sender channel for testing.
    pub fn notification_tx(mut self, tx: broadcast::Sender<Alert>) -> Self {
        self.notification_tx = Some(tx);
        self
    }

    /// Builds and initializes all application components, returning a runnable `App`.
    #[instrument(skip_all)]
    pub async fn build(self, shutdown_rx: watch::Receiver<()>) -> Result<App> {
        let config = self.config;

        // =========================================================================
        // 1. Initialize Metrics Recorder (and logging)
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
        // 2. Pre-flight Checks & Service Instantiation
        // =========================================================================
        let dns_resolver = match self.dns_resolver_override {
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
        let rule_matcher = Arc::new(RuleMatcher::load(&config.rules)?);

        let enrichment_provider = self
            .enrichment_provider_override
            .expect("Enrichment provider is now required to be passed into app::run");
        let deduplicator = Arc::new(Deduplicator::new(
            Duration::from_secs(config.deduplication.cache_ttl_seconds),
            config.deduplication.cache_size as u64,
        ));
        let in_flight_deduplicator = Arc::new(InFlightDeduplicator::new(
            Duration::from_secs(30), // Short TTL for in-flight checks
            100_000,                 // Capacity
        ));

        // =========================================================================
        // 4. Setup Output Manager
        // =========================================================================
        let output_manager = match self.output_override {
            Some(outputs) => Arc::new(OutputManager::new(outputs)),
            None => {
                let outputs: Vec<Arc<dyn Output>> =
                    vec![Arc::new(StdoutOutput::new(config.output.format.clone()))];
                Arc::new(OutputManager::new(outputs))
            }
        };

        // =========================================================================
        // 5. Create Channels for the Pipeline
        // =========================================================================
        let (internal_alerts_tx, _) = broadcast::channel::<Alert>(1000);
        let (domains_tx, domains_rx) = if let Some(rx_override) = self.domains_rx_override {
            let (tx, _) = mpsc::channel::<String>(1000);
            (tx, rx_override)
        } else {
            let (tx, rx) = mpsc::channel::<String>(1000);
            (tx, Arc::new(Mutex::new(rx)))
        };

        // =========================================================================
        // 6. Start the DNS Resolution Manager
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
        // 7. Start the CertStream Client
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
                let result = if let Some(ws) = self.websocket_override {
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
        // 8. Main Processing Loop (Worker Pool)
        // =========================================================================
        let mut worker_handles = Vec::new();
        info!("Spawning {} worker tasks...", config.concurrency);

        for i in 0..config.concurrency {
            let domains_rx = domains_rx.clone();
            let rule_matcher = rule_matcher.clone();
            let dns_manager = dns_manager.clone();
            let enrichment_provider = enrichment_provider.clone();
            let alerts_tx = internal_alerts_tx.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            let in_flight_deduplicator = in_flight_deduplicator.clone();

            let handle = tokio::spawn(async move {
                debug!("Worker {} started", i);
                loop {
                    let domain_to_process = tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            debug!("Worker {} received shutdown signal, exiting.", i);
                            None
                        }
                        domain_opt = async { domains_rx.lock().await.recv().await } => {
                            domain_opt
                        }
                    };

                    let domain = match domain_to_process {
                        Some(domain) => domain,
                        None => {
                            debug!(
                                "Domain channel closed or shutdown triggered, worker {} shutting down.",
                                i
                            );
                            break;
                        }
                    };

                    debug!(worker_id = i, domain = %domain, "Worker picked up domain");

                    if in_flight_deduplicator.is_duplicate(&domain) {
                        debug!(worker_id = i, domain = %domain, "Domain is an in-flight duplicate, skipping");
                        metrics::counter!("domains_skipped_inflight", "worker_id" => i.to_string())
                            .increment(1);
                        continue;
                    }

                    debug!(worker_id = i, domain = %domain, "Worker processing domain (pre-ignore check)");

                    if rule_matcher.is_ignored(&domain) {
                        debug!(worker_id = i, domain = %domain, "Domain is ignored by rules, skipping");
                        continue;
                    }

                    let process_fut = process_domain(
                        domain.clone(),
                        i,
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
                            debug!(worker_id = i, domain = %domain, "Worker finished processing domain");
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
        // 9. NXDOMAIN Resolution Feedback Loop
        // =========================================================================
        let nxdomain_feedback_task = {
            let alerts_tx = internal_alerts_tx.clone();
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
        // 10. Alert Deduplication and Output Task
        // =========================================================================
        let output_task = tokio::spawn(output_task_logic(
            shutdown_rx.clone(),
            internal_alerts_tx.subscribe(),
            deduplicator,
            output_manager,
            self.notification_tx,
        ));

        info!("CertWatch initialized successfully. Monitoring for domains...");

        Ok(App {
            _config: config,
            shutdown_rx,
            certstream_task,
            worker_handles,
            nxdomain_feedback_task,
            output_task,
            metrics_task,
        })
    }
}

#[instrument(skip_all)]
async fn output_task_logic(
    mut shutdown_rx: watch::Receiver<()>,
    mut alerts_rx: broadcast::Receiver<Alert>,
    deduplicator: Arc<Deduplicator>,
    output_manager: Arc<OutputManager>,
    notification_tx: Option<broadcast::Sender<Alert>>,
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
                    if let Some(tx) = &notification_tx {
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
    rule_matcher: Arc<RuleMatcher>,
    dns_manager: Arc<DnsResolutionManager>,
    enrichment_provider: Arc<dyn EnrichmentProvider>,
    alerts_tx: AlertSender,
) -> Result<()> {
    debug!("process_domain: Starting Stage 1 (pre-enrichment) filtering");
    // STAGE 1: Pre-enrichment filtering (rules only)
    let stage_1_rule_matches = {
        let _span = tracing::info_span!("stage_1_rule_matching").entered();
        let base_alert = Alert::new_minimal(&domain);
        rule_matcher.matches(&base_alert, EnrichmentLevel::None)
    };
    debug!(?stage_1_rule_matches, "process_domain: Stage 1 results");

    // If there's no match from Stage 1 rules, we can stop.
    if stage_1_rule_matches.is_empty() {
        debug!("Domain did not match any pre-enrichment rules.");
        return Ok(());
    }
    debug!("Domain matched pre-enrichment checks. Proceeding to enrichment.");

    // STAGE 2: Enrichment and Post-enrichment filtering
    debug!("process_domain: Starting DNS resolution");
    let result = dns_manager
        .resolve_with_retry(&domain, "enrichment_candidate")
        .await;
    debug!(?result, "process_domain: DNS resolution result");

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

    debug!("process_domain: Building alert and performing enrichment");
    let mut alert = build_alert(
        domain,
        "placeholder".to_string(), // Source will be replaced with combined rule names
        resolved_after_nxdomain,
        dns_info,
        enrichment_provider.clone(),
    )
    .await
    .context("Failed to build alert")?;
    debug!("process_domain: Alert built. Starting Stage 2 (post-enrichment) filtering");

    let stage_2_rule_matches = {
        let _span = tracing::info_span!("stage_2_rule_matching").entered();
        rule_matcher.matches(&alert, EnrichmentLevel::Standard)
    };
    debug!(?stage_2_rule_matches, "process_domain: Stage 2 results");

    // FINAL DECISION: Combine all matches to determine if we should alert.
    let mut all_matches_set: HashSet<String> = stage_1_rule_matches.into_iter().collect();
    all_matches_set.extend(stage_2_rule_matches);

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
    if alerts_tx.send(alert.clone()).is_err() {
        anyhow::bail!("Alerts channel closed, worker {} exiting.", worker_id);
    }
    debug!(source = %alert.source_tag, "process_domain: Alert sent to output channel");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{DnsInfo, DnsResolver},
        dns::{DnsError, DnsResolutionManager},
        enrichment::fake::FakeEnrichmentProvider,
        rules::{EnrichmentLevel, Rule, RuleExpression, RuleMatcher},
    };
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch};

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

        let rule = Rule {
            name: "Test Rule".to_string(),
            expression: RuleExpression::DomainRegex(".*".to_string()),
            required_level: EnrichmentLevel::None,
        };
        let rule_matcher = Arc::new(RuleMatcher::new_for_test(vec![rule]));

        // Act
        let result = process_domain(
            "timeout.com".to_string(),
            0,
            rule_matcher,
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
