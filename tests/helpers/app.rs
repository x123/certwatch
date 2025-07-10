#![allow(dead_code)]
//! Test helpers for running the full application instance.

use anyhow::Result;
use certwatch::{config::Config, core::Alert};
use futures::future::BoxFuture;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

/// Represents a running instance of the application for testing purposes.
use std::net::SocketAddr;

#[derive(Debug)]
pub struct TestApp {
    pub domains_tx: mpsc::Sender<String>,
    pub shutdown_tx: watch::Sender<()>,
    pub app_handle: Option<JoinHandle<Result<()>>>,
    metrics_addr: Option<SocketAddr>,
}

impl TestApp {
    pub async fn send_domain(&self, domain: &str) -> Result<()> {
        self.domains_tx.send(domain.to_string()).await?;
        Ok(())
    }

    pub fn metrics_addr(&self) -> SocketAddr {
        self.metrics_addr
            .expect("Metrics must be enabled to get the address")
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
    outputs: Option<Vec<std::sync::Arc<dyn certwatch::core::Output>>>,
    websocket: Option<Box<dyn certwatch::network::WebSocketConnection>>,
    dns_resolver: Option<std::sync::Arc<dyn certwatch::core::DnsResolver>>,
    enrichment_provider: Option<std::sync::Arc<dyn certwatch::core::EnrichmentProvider>>,
    alert_tx: Option<broadcast::Sender<Alert>>,
    slack_client: Option<Arc<dyn certwatch::notification::slack::SlackClientTrait>>,
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
            outputs: Some(vec![Arc::new(
                crate::helpers::mock_output::CountingOutput::new(),
            )]),
            websocket: None,
            dns_resolver: None,
            enrichment_provider: Some(Arc::new(crate::helpers::fake_enrichment::FakeEnrichmentProvider::new())),
            alert_tx: None,
            slack_client: None,
        }
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
        let (domains_tx, domains_rx) =
            mpsc::channel(self.config.performance.queue_capacity);

        let mut builder = certwatch::app::App::builder(self.config)
            .domains_rx_for_test(domains_rx)
            .enrichment_provider_override(
                self.enrichment_provider
                    .expect("Enrichment provider must be set in test builder"),
            );

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

        let app = builder.build(shutdown_rx).await?;
        let metrics_addr = app.metrics_addr();

        let app_future = async move { app.run().await };

        let test_app = TestApp {
            domains_tx,
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
}
