#![allow(dead_code)]
//! Test helpers for running the full application instance.

use anyhow::Result;
use certwatch::{
    app::AppBuilder, config::Config, core::Alert, internal_metrics::Metrics,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, watch, Barrier},
    task::JoinHandle,
    time::timeout,
};
use tracing::debug;
use tracing_subscriber::{fmt, EnvFilter};

/// Represents a running instance of the application for testing purposes.
use std::net::SocketAddr;

#[derive(Debug)]
pub struct TestApp {
    pub domains_tx: Option<async_channel::Sender<String>>,
    pub shutdown_tx: watch::Sender<bool>,
    pub metrics_addr: Option<SocketAddr>,
    startup_barrier: Option<Arc<Barrier>>,
    app_handle: Option<JoinHandle<Result<()>>>,
}

impl TestApp {
    pub async fn wait_for_startup(&self) -> Result<()> {
        if let Some(barrier) = &self.startup_barrier {
            barrier.wait().await;
        }
        Ok(())
    }

    pub async fn send_domain(&self, domain: &str) -> Result<()> {
        debug!("TestApp: Sending domain: {}", domain);
        self.domains_tx
            .as_ref()
            .expect("domains_tx is only available when using with_test_domains_channel")
            .send(domain.to_string())
            .await?;
        Ok(())
    }

    pub fn metrics_addr(&self) -> Option<SocketAddr> {
        self.metrics_addr
    }

    pub fn close_domains_channel(&mut self) {
        if let Some(tx) = self.domains_tx.take() {
            drop(tx);
        }
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.shutdown_tx.send(true).unwrap();
        if let Some(handle) = self.app_handle.take() {
            match timeout(Duration::from_secs(10), handle).await {
                Ok(Ok(Ok(_))) => Ok(()),
                Ok(Ok(Err(e))) => Err(anyhow::anyhow!("App run failed: {}", e)),
                Ok(Err(e)) => Err(anyhow::anyhow!("App task panicked: {}", e)),
                Err(_) => Err(anyhow::anyhow!("Shutdown timed out")),
            }
        } else {
            Ok(())
        }
    }
}

/// A builder for creating `TestApp` instances with specific configurations.
#[derive(Default)]
pub struct TestAppBuilder {
    pub config: Config,
    domains_tx_for_test: Option<async_channel::Sender<String>>,
    domains_rx_for_test: Option<async_channel::Receiver<String>>,
    outputs: Option<Vec<std::sync::Arc<dyn certwatch::core::Output>>>,
    websocket: Option<Box<dyn certwatch::network::WebSocketConnection>>,
    dns_resolver: Option<std::sync::Arc<dyn certwatch::core::DnsResolver>>,
    enrichment_provider: Option<std::sync::Arc<dyn certwatch::core::EnrichmentProvider>>,
    alert_tx: Option<broadcast::Sender<Alert>>,
    slack_client: Option<Arc<dyn certwatch::notification::slack::SlackClientTrait>>,
    metrics: Option<Metrics>,
    skip_health_check: bool,
    startup_barrier: Option<Arc<Barrier>>,
}

impl TestAppBuilder {
    pub fn new() -> Self {
        // Use the registry to build a layered subscriber, which is the most robust
        // way to initialize tracing for tests.
        use tracing_subscriber::prelude::*;
        let _ = tracing_subscriber::registry()
            .with(fmt::layer().with_writer(std::io::stderr))
            .with(EnvFilter::from_default_env())
            .try_init();
        let mut config = Config::default();
        // Disable network-dependent features for default tests
        config.network.certstream_url = "ws://127.0.0.1:12345".to_string(); // Mock URL
        config.output.slack = None;
        // Set a path for the ASN database, even if it's empty, to satisfy the check
        config.enrichment.asn_tsv_path = Some("/tmp/empty.tsv".into());

        Self {
            config,
            domains_tx_for_test: None,
            domains_rx_for_test: None,
            outputs: Some(vec![Arc::new(
                crate::helpers::mock_output::CountingOutput::new(1), // Default target count for general tests
            )]),
            websocket: None,
            dns_resolver: None,
            enrichment_provider: Some(Arc::new(crate::helpers::fake_enrichment::FakeEnrichmentProvider::new())),
            alert_tx: None,
            slack_client: None,
            metrics: None,
            skip_health_check: false,
            startup_barrier: Some(Arc::new(Barrier::new(2))),
        }
    }

    /// Sets up a direct MPSC channel for domain injection, bypassing the network client.
    /// The sender part of the channel is returned in the `TestApp` struct.
    pub fn with_test_domains_channel(mut self) -> Self {
        let (tx, rx) = async_channel::unbounded();
        self.domains_tx_for_test = Some(tx);
        self.domains_rx_for_test = Some(rx);
        self
    }

    pub fn with_alert_tx(mut self, alert_tx: broadcast::Sender<Alert>) -> Self {
        self.alert_tx = Some(alert_tx);
        self
    }

    pub fn with_outputs(
        mut self,
        outputs: Vec<std::sync::Arc<dyn certwatch::core::Output>>,
    ) -> Self {
        self.outputs = Some(outputs);
        self
    }

    pub fn with_websocket(
        mut self,
        ws: Box<dyn certwatch::network::WebSocketConnection>,
    ) -> Self {
        self.websocket = Some(ws);
        self
    }


    pub fn with_dns_resolver(
        mut self,
        resolver: std::sync::Arc<dyn certwatch::core::DnsResolver>,
    ) -> Self {
        self.dns_resolver = Some(resolver);
        self
    }

    pub fn with_enrichment_provider(
        mut self,
        provider: std::sync::Arc<dyn certwatch::core::EnrichmentProvider>,
    ) -> Self {
        self.enrichment_provider = Some(provider);
        self
    }

    pub fn with_failing_enrichment(self, fail: bool) -> Self {
        if let Some(provider_arc) = &self.enrichment_provider {
            if let Some(fake_provider) = provider_arc
                .as_any()
                .downcast_ref::<crate::helpers::fake_enrichment::FakeEnrichmentProvider>()
            {
                fake_provider.set_fail(fail);
            }
        }
        self
    }


    pub fn with_config_modifier(mut self, modifier: impl FnOnce(&mut Config)) -> Self {
        modifier(&mut self.config);
        self
    }

    pub fn with_performance_config(mut self, perf_config: certwatch::config::PerformanceConfig) -> Self {
        self.config.performance = perf_config;
        self
    }

    pub fn with_rules(mut self, rules_content: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("rules.yml");
        std::fs::write(&file_path, rules_content).unwrap();
        self.config.rules.rule_files = Some(vec![file_path]);
        // Intentionally leak the tempdir to keep the file alive for the test run.
        std::mem::forget(dir);
        self
    }

    /// Builds the application components and spawns it.
    /// Returns the TestApp handle.
    pub fn with_slack_notification(
        mut self,
        client: Arc<dyn certwatch::notification::slack::SlackClientTrait>,
        _channel: &str,
    ) -> Self {
        self.slack_client = Some(client);
        self.config.output.slack = Some(certwatch::config::SlackConfig {
            enabled: Some(true),
            webhook_url: Some("https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX".to_string()),
            batch_size: Some(10),
            batch_timeout_seconds: Some(1),
        });
        self
    }

    pub fn with_metrics(mut self) -> Self {
        self.config.metrics.enabled = true;
        self
    }

    pub fn with_metrics_override(mut self, metrics: Metrics) -> Self {
        self.config.metrics.enabled = true;
        self.metrics = Some(metrics);
        self
    }

    pub async fn build(mut self) -> Result<TestApp> {
        // Ensure the dummy ASN file exists to prevent startup errors
        if let Some(path) = &self.config.enrichment.asn_tsv_path {
            if !path.exists() {
                let parent = path.parent().unwrap();
                tokio::fs::create_dir_all(parent).await?;
                tokio::fs::write(path, "").await?;
            }
        }

        if self.config.metrics.enabled {
            // Use a random port for the metrics server in tests
            self.config.metrics.listen_address = "127.0.0.1:0".parse().unwrap();
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        debug!("TestAppBuilder: Calling AppBuilder::build");
        let mut builder = AppBuilder::new(self.config)
            .enrichment_provider_override(self.enrichment_provider)
            .skip_health_check(self.skip_health_check);

        if let Some(rx) = self.domains_rx_for_test.take() {
            builder = builder.domains_rx_for_test(rx);
        }
        if let Some(outputs) = self.outputs.take() {
            builder = builder.output_override(outputs);
        }
        if let Some(ws) = self.websocket.take() {
            builder = builder.websocket_override(ws);
        }
        if let Some(resolver) = self.dns_resolver.take() {
            builder = builder.dns_resolver_override(resolver);
        }
        if let Some(tx) = self.alert_tx.take() {
            builder = builder.notification_tx(tx);
        }
        if let Some(sc) = self.slack_client.take() {
            builder = builder.slack_client_override(sc);
        }
        if let Some(metrics) = self.metrics.take() {
            builder = builder.metrics_override(metrics);
        }

        if let Some(barrier) = self.startup_barrier.clone() {
            builder = builder.startup_barrier(barrier);
        }

        let app = builder.build(shutdown_rx).await?;
        let metrics_addr = app.metrics_addr();

        let app_handle = tokio::spawn(app.run());

        let test_app = TestApp {
            domains_tx: self.domains_tx_for_test,
            shutdown_tx,
            metrics_addr,
            startup_barrier: self.startup_barrier,
            app_handle: Some(app_handle),
        };

        Ok(test_app)
    }

    pub async fn start(self) -> Result<TestApp> {
        self.build().await
    }

    pub fn with_skipped_health_check(mut self) -> Self {
        self.skip_health_check = true;
        self
    }

    pub fn with_disabled_periodic_health_check(mut self) -> Self {
        self.config.dns.health.enabled = false;
        self
    }
}
