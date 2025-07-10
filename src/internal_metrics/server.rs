//! # Metrics Server
//!
//! This module defines the `MetricsServer`, which is responsible for running
//! an `axum`-based web server to expose the collected metrics to a Prometheus
//! scraper.
//!
//! The server provides a single endpoint, `/metrics`, which, when scraped,
//! returns the current state of all registered metrics in the Prometheus
//! exposition format.
//!
//! The server is designed for graceful shutdown, listening to a signal from
//! the main application to stop serving requests and terminate cleanly.

use axum::{routing::get, Router};
use log::error;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::{net::TcpListener, sync::watch};
use tokio::task::JoinHandle;

/// A server that exposes metrics to a Prometheus scraper.
///
/// This struct encapsulates the `axum` server and its associated task handle,
/// providing a clean interface for managing the server's lifecycle.
pub struct MetricsServer {
    /// The `JoinHandle` for the spawned `axum` server task.
    /// This can be used by the main application to await the server's
    /// graceful shutdown.
    pub task: JoinHandle<()>,
}

impl MetricsServer {
    /// Creates a new `MetricsServer` and spawns it in a background task.
    ///
    /// # Arguments
    ///
    /// * `listener` - A `TcpListener` that has already been bound to an address.
    /// * `handle` - A `PrometheusHandle` used to render the metrics.
    /// * `shutdown_rx` - A `watch::Receiver` for the graceful shutdown signal.
    pub fn new(
        listener: TcpListener,
        handle: PrometheusHandle,
        mut shutdown_rx: watch::Receiver<()>,
    ) -> Self {
        let app = Router::new().route(
            "/metrics",
            get(move || async move {
                handle.render()
            }),
        );

        let task = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app.into_make_service())
                .with_graceful_shutdown(async move {
                    shutdown_rx.changed().await.ok();
                })
                .await
            {
                error!("Metrics server error: {}", e);
            }
        });

        Self { task }
    }
}