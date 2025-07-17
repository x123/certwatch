#![allow(dead_code)]
//! Test helpers for running the full application instance.

use anyhow::Result;
use certwatch::{
    app::AppBuilder, config::Config, core::Alert, internal_metrics::Metrics,
};
use futures::future::BoxFuture;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
    time::timeout,
};

/// Represents a running instance of the application for testing purposes.
use std::net::SocketAddr;

#[derive(Debug)]
pub struct TestApp {
    pub domains_tx: Option<async_channel::Sender<String>>,
    pub shutdown_tx: watch::Sender<()>,
    pub app_handle: Option<JoinHandle<Result<()>>>,
    metrics_addr: Option<SocketAddr>,
}

impl TestApp {
    pub async fn send_domain(&self, domain: &str) -> Result<()> {
        self.domains_tx
            .as_ref()
            .expect("domains_tx is only available when using with_test_domains_channel")
            .send(domain.to_string())
            .await?;
        Ok(())
    }

    pub fn metrics_addr(&self) -> SocketAddr {
        self.metrics_addr
            .expect("Metrics must be enabled to get the address")
    }

    pub fn close_domains_channel(&mut self) {
        if let Some(tx) = self.domains_tx.take() {
            drop(tx);
        }
    }

    /// Shuts down the application and waits for it to terminate.
    /// Fails if the application does not shut down within the specified timeout.
    pub async fn shutdown(self, timeout_duration: Duration) -> Result<()> {
        // Send the shutdown signal
        self.shutdown_tx
            .send(())
            .expect("Failed to send shutdown signal");

        // Wait for the application to terminate
        if let Some(handle) = self.app_handle {
            match timeout(timeout_duration, handle).await {
                Ok(Ok(_)) => Ok(()), // App finished successfully
                Ok(Err(e)) => Err(e.into()), // App returned an error
                Err(_) => Err(anyhow::anyhow!("App failed to shut down within the timeout")),
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
}

impl TestAppBuilder {
    pub fn new() -> Self {
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
                crate::helpers::mock_output::CountingOutput::new(),
            )]),
            websocket: None,
            dns_resolver: None,
            enrichment_provider: Some(Arc::new(crate::helpers::fake_enrichment::FakeEnrichmentProvider::new())),
            alert_tx: None,
            slack_client: None,
            metrics: None,
            skip_health_check: false,
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

    pub async fn with_rules(mut self, rules_content: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("rules.yml");
        tokio::fs::write(&file_path, rules_content).await.unwrap();
        self.config.rules.rule_files = Some(vec![file_path]);
        // Intentionally leak the tempdir to keep the file alive for the test run.
        std::mem::forget(dir);
        self
    }

    /// Builds the application components but does not spawn it.
    /// Returns the TestApp handle and a future that runs the app.
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

    pub async fn build(mut self) -> Result<(TestApp, BoxFuture<'static, Result<()>>)> {
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

        let (shutdown_tx, shutdown_rx) = watch::channel(());

        let mut builder = AppBuilder::new(self.config)
            .enrichment_provider_override(self.enrichment_provider)
            .skip_health_check(self.skip_health_check);

        if let Some(rx) = self.domains_rx_for_test {
            builder = builder.domains_rx_for_test(rx);
        }
        if let Some(outputs) = self.outputs {
            builder = builder.output_override(outputs);
        }
        if let Some(ws) = self.websocket {
            builder = builder.websocket_override(ws);
        }
        if let Some(resolver) = self.dns_resolver {
            builder = builder.dns_resolver_override(resolver);
        }
        if let Some(tx) = self.alert_tx {
            builder = builder.notification_tx(tx);
        }
        if let Some(sc) = self.slack_client {
            builder = builder.slack_client_override(sc);
        }
        if let Some(metrics) = self.metrics {
            builder = builder.metrics_override(metrics);
        }

        let app = builder.build(shutdown_rx).await?;
        let metrics_addr = app.metrics_addr();
        let app_future = async move { app.run().await };

        let test_app = TestApp {
            domains_tx: self.domains_tx_for_test,
            shutdown_tx,
            app_handle: None, // The app is not running yet
            metrics_addr,
        };

        Ok((test_app, Box::pin(app_future)))
    }

    pub async fn start(self) -> Result<TestApp> {
        let (mut test_app, app_future) = self.build().await?;
        let handle = tokio::spawn(app_future);
        test_app.app_handle = Some(handle);
        Ok(test_app)
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
