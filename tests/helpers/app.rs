#![allow(dead_code)]
//! Test helpers for running the full application instance.

use anyhow::Result;
use certwatch::{config::Config, core::Alert};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

/// Represents a running instance of the application for testing purposes.
#[derive(Debug)]
pub struct TestApp {
    pub domains_tx: mpsc::Sender<String>,
    pub shutdown_tx: watch::Sender<()>,
    pub app_handle: JoinHandle<Result<()>>,
}

impl TestApp {
    /// Shuts down the application and waits for it to terminate.
    /// Fails if the application does not shut down within the specified timeout.
    pub async fn shutdown(self, timeout_duration: Duration) -> Result<()> {
        // Send the shutdown signal
        self.shutdown_tx.send(()).expect("Failed to send shutdown signal");

        // Wait for the application to terminate
        match timeout(timeout_duration, self.app_handle).await {
            Ok(Ok(_)) => Ok(()), // App finished successfully
            Ok(Err(e)) => Err(e.into()), // App returned an error
            Err(_) => Err(anyhow::anyhow!("App failed to shut down within the timeout")),
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
}

impl TestAppBuilder {
    pub fn new() -> Self {
        let mut config = Config::default();
        // Disable network-dependent features for default tests
        config.network.certstream_url = "ws://127.0.0.1:12345".to_string(); // Mock URL
        config.output.slack = None;
        config.metrics.log_metrics = false;
        // Use a minimal pattern file that won't exist, so no matching occurs
        config.matching.pattern_files = vec!["/tmp/nonexistent-patterns.txt".into()];
        // Set a path for the ASN database, even if it's empty, to satisfy the check
        config.enrichment.asn_tsv_path = Some("/tmp/empty.tsv".into());


        Self {
            config,
            outputs: Some(vec![Arc::new(
                crate::helpers::mock_output::FailableMockOutput::new(),
            )]),
            websocket: None,
            dns_resolver: None,
            enrichment_provider: Some(Arc::new(crate::helpers::fake_enrichment::FakeEnrichmentProvider::new())),
            alert_tx: None,
        }
    }

    pub fn with_alert_tx(mut self, alert_tx: broadcast::Sender<Alert>) -> Self {
        self.alert_tx = Some(alert_tx);
        self
    }

    pub fn with_outputs(mut self, outputs: Vec<std::sync::Arc<dyn certwatch::core::Output>>) -> Self {
        self.outputs = Some(outputs);
        self
    }

    pub fn with_websocket(mut self, ws: Box<dyn certwatch::network::WebSocketConnection>) -> Self {
        self.websocket = Some(ws);
        self
    }

    pub fn with_pattern_files(mut self, files: Vec<std::path::PathBuf>) -> Self {
        self.config.matching.pattern_files = files;
        self
    }

    pub fn with_dns_resolver(mut self, resolver: std::sync::Arc<dyn certwatch::core::DnsResolver>) -> Self {
        self.dns_resolver = Some(resolver);
        self
    }

    pub fn with_enrichment_provider(mut self, provider: std::sync::Arc<dyn certwatch::core::EnrichmentProvider>) -> Self {
        self.enrichment_provider = Some(provider);
        self
    }

    pub fn with_config_modifier(mut self, modifier: impl FnOnce(&mut Config)) -> Self {
        modifier(&mut self.config);
        self
    }

    /// Builds and spawns the application in a background task.
    pub async fn build(self) -> Result<TestApp> {
        // Ensure the dummy ASN file exists to prevent startup errors
        if let Some(path) = &self.config.enrichment.asn_tsv_path {
            if !path.exists() {
                let parent = path.parent().unwrap();
                tokio::fs::create_dir_all(parent).await?;
                tokio::fs::write(path, "").await?;
            }
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (domains_tx, _) = mpsc::channel(self.config.performance.queue_capacity);

        let domains_tx_clone = domains_tx.clone();
        let app_handle = tokio::spawn(async move {
            certwatch::app::run(
                self.config,
                shutdown_rx,
                Some(domains_tx_clone),
                self.outputs,
                self.websocket,
                self.dns_resolver,
                self.enrichment_provider,
                self.alert_tx,
            )
            .await
        });

        Ok(TestApp {
            domains_tx,
            shutdown_tx,
            app_handle,
        })
    }
}