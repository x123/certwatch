#![allow(dead_code)]
//! Test helpers for running the full application instance.

use anyhow::Result;
use certwatch::config::Config;
use std::time::Duration;
use tokio::{task::JoinHandle, time::timeout};
use tokio::sync::watch;

/// Represents a running instance of the application for testing purposes.
#[derive(Debug)]
pub struct TestApp {
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


        Self { config, outputs: None, websocket: None, dns_resolver: None, enrichment_provider: None }
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

        let app_handle = tokio::spawn(async move {
            certwatch::app::run(self.config, shutdown_rx, self.outputs, self.websocket, self.dns_resolver, self.enrichment_provider).await
        });

        Ok(TestApp {
            shutdown_tx,
            app_handle,
        })
    }
}