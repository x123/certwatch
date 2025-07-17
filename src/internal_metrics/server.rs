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
use tracing::trace;
use std::future::Future;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// A server that exposes metrics to a Prometheus scraper.
///
/// This struct encapsulates the `axum` server and its associated task handle,
/// providing a clean interface for managing the server's lifecycle.
pub struct MetricsServer {
    listener: TcpListener,
    prom_handle: PrometheusHandle,
    shutdown_rx: watch::Receiver<bool>,
}

impl MetricsServer {
    /// Creates a new `MetricsServer` but does not spawn it.
    ///
    /// # Arguments
    ///
    /// * `listener` - A `TcpListener` that has already been bound to an address.
    /// * `prom_handle` - A `PrometheusHandle` used to render the metrics.
    /// * `shutdown_rx` - A watch channel receiver for graceful shutdown.
    pub fn new(
        listener: TcpListener,
        prom_handle: PrometheusHandle,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            listener,
            prom_handle,
            shutdown_rx,
        }
    }

    /// Returns a future that runs the server until a shutdown signal is received.
    pub fn run(mut self) -> impl Future<Output = ()> {
        let app = Router::new().route("/metrics", get(move || async move { self.prom_handle.render() }));

        async move {
            tokio::select! {
                biased;
                _ = self.shutdown_rx.changed() => {
                    trace!("Metrics server received shutdown signal via select.");
                }
                result = axum::serve(self.listener, app.into_make_service()) => {
                    if let Err(e) = result {
                        // This error is expected during graceful shutdown when the server is dropped.
                        if !e.to_string().contains("operation was canceled") {
                            error!("Metrics server error: {}", e);
                        }
                    }
                }
            }
            trace!("Metrics server task finished.");
        }
    }
}