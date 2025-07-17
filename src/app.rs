//! The main application logic, decoupled from the entry point.

use crate::{
    build_alert,
    config::Config,
    core::{Alert, DnsResolver, EnrichmentProvider, Output},
    deduplication::Deduplicator,
    dns::{DnsHealth, DnsResolutionManager, HickoryDnsResolver},
    internal_metrics::{Metrics, MetricsBuilder},
    network::{CertStreamClient, WebSocketConnection},
    notification::slack::SlackClientTrait,
    outputs::{OutputManager, StdoutOutput},
    rules::{EnrichmentLevel, RuleLoader, RuleMatcher},
    utils::heartbeat::run_heartbeat,
};
use anyhow::Result;
use async_channel::Receiver;
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info, instrument, trace};
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;


/// A handle to the running application, containing all its task handles.
pub struct App {
    shutdown_rx: watch::Receiver<()>,
    certstream_task: Option<JoinHandle<()>>,
    dns_manager_task: Option<JoinHandle<()>>,
    rules_worker_handles: Vec<JoinHandle<()>>,
    output_task: JoinHandle<()>,
    metrics_server_task: Option<JoinHandle<()>>,
    metrics_addr: Option<std::net::SocketAddr>,
}

impl App {
    /// Creates a new `AppBuilder` to construct an `App`.
    pub fn builder(config: Config) -> AppBuilder {
        AppBuilder::new(config)
    }

    pub fn metrics_addr(&self) -> Option<std::net::SocketAddr> {
        self.metrics_addr
    }

    /// Waits for the shutdown signal and then gracefully shuts down all tasks.
    pub async fn run(self) -> Result<()> {
        let mut shutdown_rx = self.shutdown_rx;
        // Wait for the external shutdown signal
        shutdown_rx.changed().await.ok();
        info!("Shutdown signal received in run function. Waiting for tasks to complete...");

        // Wait for all tasks to complete
        if let Some(handle) = self.certstream_task {
            if let Err(e) = handle.await {
                error!("CertStream task panicked: {:?}", e);
            }
        }
        if let Some(handle) = self.dns_manager_task {
            if let Err(e) = handle.await {
                error!("DNS manager task panicked: {:?}", e);
            }
        }
        for handle in self.rules_worker_handles {
            if let Err(e) = handle.await {
                error!("Rules worker task panicked: {:?}", e);
            }
        }
        if let Err(e) = self.output_task.await {
            error!("Output task panicked: {:?}", e);
        }
        if let Some(handle) = self.metrics_server_task {
            if let Err(e) = handle.await {
                error!("Metrics server task panicked: {:?}", e);
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
    domains_rx_for_test: Option<Receiver<String>>,
    output_override: Option<Vec<Arc<dyn Output>>>,
    websocket_override: Option<Box<dyn WebSocketConnection>>,
    dns_resolver_override: Option<Arc<dyn DnsResolver>>,
    enrichment_provider_override: Option<Arc<dyn EnrichmentProvider>>,
    notification_tx: Option<broadcast::Sender<Alert>>,
    slack_client_override: Option<Arc<dyn SlackClientTrait>>,
    metrics_override: Option<Metrics>,
    skip_health_check: bool,
}

impl AppBuilder {
    /// Creates a new `AppBuilder` with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            domains_rx_for_test: None,
            output_override: None,
            websocket_override: None,
            dns_resolver_override: None,
            enrichment_provider_override: None,
            notification_tx: None,
            slack_client_override: None,
            metrics_override: None,
            skip_health_check: false,
        }
    }

    /// Skips the initial DNS health check during application startup.
    pub fn skip_health_check(mut self, skip: bool) -> Self {
        self.skip_health_check = skip;
        self
    }

    /// Overrides the domain receiver channel for testing.
    pub fn domains_rx_for_test(mut self, rx: Receiver<String>) -> Self {
        self.domains_rx_for_test = Some(rx);
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
    pub fn enrichment_provider_override(
        mut self,
        provider: Option<Arc<dyn EnrichmentProvider>>,
    ) -> Self {
        self.enrichment_provider_override = provider;
        self
    }

    /// Overrides the notification sender channel for testing.
    pub fn notification_tx(mut self, tx: broadcast::Sender<Alert>) -> Self {
        self.notification_tx = Some(tx);
        self
    }

    /// Overrides the Slack client for testing.
    pub fn slack_client_override(mut self, client: Arc<dyn SlackClientTrait>) -> Self {
        self.slack_client_override = Some(client);
        self
    }

    /// Overrides the metrics system for testing.
    pub fn metrics_override(mut self, metrics: Metrics) -> Self {
        self.metrics_override = Some(metrics);
        self
    }

    /// Builds and initializes all application components, returning a runnable `App`.
    #[instrument(skip_all)]
    pub async fn build(self, shutdown_rx: watch::Receiver<()>) -> Result<App> {
        let config = self.config;

        // =========================================================================
        // 1. Initialize Metrics
        // =========================================================================
        let (metrics, metrics_server_info) = match self.metrics_override {
            Some(m) => (m, None),
            None => MetricsBuilder::new(config.metrics.clone()).build(shutdown_rx.clone()),
        };
        let metrics = Arc::new(metrics);

        let (metrics_server_task, metrics_addr) = if let Some((server, addr)) = metrics_server_info
        {
            (Some(server.task), Some(addr))
        } else {
            (None, None)
        };


        // =========================================================================
        // 2. Pre-flight Checks & Service Instantiation
        // =========================================================================
        let (dns_resolver, dns_health_monitor) = match self.dns_resolver_override {
            Some(resolver) => {
                let health_monitor = DnsHealth::new(
                    config.dns.health.clone(),
                    resolver.clone(),
                    shutdown_rx.clone(),
                    metrics.clone(),
                    vec![], // No nameservers available in override mode
                );
                (resolver, health_monitor)
            }
            None => {
                let (resolver, nameservers) =
                    HickoryDnsResolver::from_config(&config.dns, metrics.clone())?;
                let resolver = Arc::new(resolver) as Arc<dyn DnsResolver>;
                let health_monitor = DnsHealth::new(
                    config.dns.health.clone(),
                    resolver.clone(),
                    shutdown_rx.clone(),
                    metrics.clone(),
                    nameservers,
                );
                (resolver, health_monitor)
            }
        };

        if !self.skip_health_check {
            DnsHealth::startup_check(&*dns_resolver, &config.dns.health).await?;
        }

        // =========================================================================
        // 3. Instantiate Remaining Services
        // =========================================================================
        let rule_set = RuleLoader::load_from_files(&config.rules)?;
        metrics.set_rules_loaded_count(rule_set.rules.len() as u64);
        let rule_matcher = Arc::new(RuleMatcher::new(rule_set, metrics.clone())?);

        let enrichment_provider = match self.enrichment_provider_override {
            Some(provider) => Some(provider),
            None => {
                if let Some(path) = &config.enrichment.asn_tsv_path {
                    let provider =
                        crate::enrichment::TsvAsnLookup::new_from_path(path)?;
                    Some(Arc::new(provider) as Arc<dyn EnrichmentProvider>)
                } else {
                    None
                }
            }
        };
        let cache_ttl_seconds = config.deduplication.cache_ttl_seconds;
        let cache_size = config.deduplication.cache_size;
        debug!(cache_ttl_seconds, cache_size, "Initializing deduplicator");
        let deduplicator = Arc::new(Deduplicator::new(
            Duration::from_secs(cache_ttl_seconds),
            cache_size as u64,
            metrics.clone(),
        ));

        // =========================================================================
        // 4. Setup Output Manager
        // =========================================================================
        let output_manager = match self.output_override {
            Some(outputs) => Arc::new(OutputManager::new(outputs, metrics.clone())),
            None => {
                let output_format = config.output.format.clone().unwrap_or_default();
                debug!(?output_format, "Initializing StdoutOutput");
                let outputs: Vec<Arc<dyn Output>> =
                    vec![Arc::new(StdoutOutput::new(output_format))];
                Arc::new(OutputManager::new(outputs, metrics.clone()))
            }
        };

        // =========================================================================
        // 5. Create Channels for the Pipeline
        // =========================================================================

        // If a test receiver is provided, use it. Otherwise, create a new channel
        // and spawn the CertStream client to populate it.
        let (domains_rx, certstream_task) = if let Some(rx) = self.domains_rx_for_test {
            (rx, None)
        } else {
            let (tx, rx) = async_channel::unbounded();
            let certstream_url = config.network.certstream_url.clone();
            let sample_rate = config.network.sample_rate;
            let allow_invalid_certs = config.network.allow_invalid_certs;
            debug!(
                certstream_url,
                sample_rate,
                allow_invalid_certs,
                "Initializing CertStream client"
            );
            let certstream_client =
                CertStreamClient::new(
                    certstream_url,
                    tx,
                    sample_rate,
                    allow_invalid_certs,
                    metrics.clone(),
                );

            let task = {
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
            (rx, Some(task))
        };

        // =========================================================================
        // 6. Build the Pipeline
        // =========================================================================
        let (alerts_tx, alerts_rx) = async_channel::unbounded();

        // =========================================================================
        // STAGE 2: DNS Resolution Manager
        // =========================================================================
        let (dns_manager, resolved_rx) = DnsResolutionManager::start(
            dns_resolver.clone(),
            config.dns.retry_config.clone(),
            &config.performance,
            dns_health_monitor.clone(),
            shutdown_rx.clone(),
            metrics.clone(),
        );

        // This task forwards domains from the certstream to the DNS manager
        let dns_manager_task = {
            let mut shutdown_rx = shutdown_rx.clone();
            let metrics = metrics.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            info!("DNS domain forwarder received shutdown signal.");
                            break;
                        }
                        Ok(domain) = domains_rx.recv() => {
                            let start_time = std::time::Instant::now();
                            metrics::gauge!("domains_queued").decrement(1.0);
                            metrics.domains_processed_total.increment(1);
                            if let Err(e) = dns_manager.resolve(domain, "certstream".to_string(), start_time.into()) {
                                trace!("Failed to send domain to DNS manager: {}", e);
                            }
                        }
                        else => {
                            info!("Domain channel closed, DNS forwarder shutting down.");
                            break;
                        }
                    }
                }
            })
        };

        // For now, the main pipeline for resolved domains is not connected,
        // as the manager handles everything internally. We create a dummy channel.
        // The resolved_rx from the DnsResolutionManager is now the input to the rules engine.

        // =========================================================================
        // STAGE 3: Rule Matching & Enrichment
        // =========================================================================
        let mut rules_worker_handles = Vec::new();
        let rules_worker_concurrency = config.performance.rules_worker_concurrency;
        info!(
            "Spawning {} rules worker tasks...",
            rules_worker_concurrency
        );

        for i in 0..rules_worker_concurrency {
            let mut shutdown_rx = shutdown_rx.clone();
            let resolved_rx = resolved_rx.clone();
            let alerts_tx = alerts_tx.clone();
            let rule_matcher = rule_matcher.clone();
            let enrichment_provider = enrichment_provider.clone();
            let metrics = metrics.clone(); // Clone metrics for the worker
            let handle = tokio::spawn(async move {
                trace!("Rules Worker {} started", i);
                loop {
                    let resolved_domain = tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            trace!("Rules Worker {} received shutdown signal, exiting.", i);
                            None
                        }
                        resolved_opt = resolved_rx.recv() => {
                            resolved_opt.ok()
                        }
                    };

                    if let Some((domain, dns_info, start_time)) = resolved_domain {
                        let loop_start_time = std::time::Instant::now();
                        trace!(worker_id = i, domain = %domain, "Rules worker picked up domain");

                        // Build a base alert. The source_tag will be overwritten by the
                        // rule name for each match.
                        let base_alert = match build_alert(
                            domain,
                            vec![], // Placeholder, will be replaced
                            false,
                            dns_info,
                            enrichment_provider.clone(),
                            Some(start_time.into()),
                        )
                        .await
                        {
                            Ok(alert) => alert,
                            Err(e) => {
                                error!("Failed to build alert: {}", e);
                                continue;
                            }
                        };
                        let _start_time = start_time; // Use the variable to suppress the warning

                        // A single alert can match rules at multiple stages.
                        // We collect all matches.
                        let all_matches = {
                            let mut matches =
                                rule_matcher.matches(&base_alert, EnrichmentLevel::None);
                            matches.extend(rule_matcher.matches(
                                &base_alert,
                                EnrichmentLevel::Standard,
                            ));
                            matches
                        };

                        if !all_matches.is_empty() {
                            let mut alert = base_alert;
                            alert.source_tag = all_matches;
                            if alerts_tx.send(alert).await.is_err() {
                                error!("Failed to send alert to output stage, worker shutting down.");
                                // If the channel is closed, we can't continue.
                                break;
                            }
                        }

                        metrics.worker_loop_iteration_duration_seconds.record(loop_start_time.elapsed());

                    } else {
                        trace!("Resolved channel closed, rules worker {} shutting down.", i);
                        break;
                    }
                }
            });
            rules_worker_handles.push(handle);
        }

        // =========================================================================
        // 10. Alert Deduplication and Output Task
        // =========================================================================
        let output_task = tokio::spawn(output_task_logic(
            shutdown_rx.clone(),
            alerts_rx,
            deduplicator,
            output_manager,
            self.notification_tx,
        ));

        info!("CertWatch initialized successfully. Monitoring for domains...");

        Ok(App {
            shutdown_rx,
            certstream_task,
            dns_manager_task: Some(dns_manager_task),
            rules_worker_handles,
            output_task,
            metrics_server_task,
            metrics_addr,
        })
    }
}

#[instrument(skip_all)]
async fn output_task_logic(
    mut shutdown_rx: watch::Receiver<()>,
    alerts_rx: Receiver<Alert>,
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

                if let Some(start_time) = alert.processing_start_time {
                    let duration = start_time.elapsed().as_secs_f64();
                    metrics::histogram!("processing_duration_seconds").record(duration);
                }

                let start_time = std::time::Instant::now();
                let is_dupe = deduplicator.is_duplicate(&alert).await;
                metrics::histogram!("deduplication_duration_seconds")
                    .record(start_time.elapsed().as_secs_f64());

                if !is_dupe {
                    debug!("Domain {} is not a duplicate. Processing.", &alert.domain);

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



