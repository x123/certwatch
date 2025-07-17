//! # Internal Metrics Module
//!
//! This module provides the core infrastructure for collecting and exposing
//! application metrics, conforming to the design specified in the metrics
//! architecture document.
//!
//! ## Components:
//!
//! - **`MetricsBuilder`**: The entry point for initializing the metrics system.
//!   It sets up the Prometheus recorder, spawns the metrics server, and
//!   constructs the `Metrics` handle.
//!
//! - **`Metrics`**: A lightweight, cloneable struct that serves as the public
//!   API for the rest of the application to interact with the metrics system.
//!   It provides high-level methods for updating specific, predefined metrics.
//!
//! - **`MetricsServer`**: (Defined in `server.rs`) An `axum`-based web server
//!   that exposes the `/metrics` endpoint for Prometheus to scrape.
//!
//! - **`SystemCollector`**: (Defined in `system.rs`) A background task that
//!   periodically collects and updates system-level metrics (CPU, memory).

use crate::config::MetricsConfig;
use crate::internal_metrics::server::MetricsServer;
use crate::internal_metrics::system::SystemCollector;
use log::error;
use metrics::{Counter, Histogram, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// The public API for the metrics system.
///
/// This struct holds cloneable handles to the metrics collectors.
#[derive(Clone)]
pub struct Metrics {
    pub domains_ingested_total: Counter,
    pub domains_processed_total: Counter,
    pub domains_ignored_total: Counter,
    // This metric is now handled directly via macro at the call site.
    pub deduplicated_alerts_total: Counter,
    // Gauges and Histograms can be added here as needed
    pub worker_loop_iteration_duration_seconds: Histogram,
    pub dns_worker_scheduling_delay_seconds: Histogram,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics").finish_non_exhaustive()
    }
}

impl Metrics {
    /// Creates a new `Metrics` instance and registers descriptions for all
    /// supported metrics with the global recorder.
    pub fn new() -> Self {
        // Descriptions (for Prometheus)
        metrics::describe_counter!("domains_ingested_total", Unit::Count, "Total number of domains received from the websocket before any sampling.");
        metrics::describe_counter!("domains_processed_total", Unit::Count, "Total number of domains received from the input source and processed by a worker.");
        metrics::describe_counter!("domains_ignored_total", Unit::Count, "Total number of domains that were ignored based on the ignore rules.");
        metrics::describe_counter!("alerts_sent_total", Unit::Count, "Total number of alerts successfully sent, labeled by output.");
        metrics::describe_counter!("deduplicated_alerts_total", Unit::Count, "Total number of alerts that were suppressed by the deduplication filter.");
        metrics::describe_gauge!("active_workers", Unit::Count, "The current number of worker tasks actively processing domains.");
        metrics::describe_gauge!("domains_queued", Unit::Count, "The current number of domains in the queue awaiting processing.");
        metrics::describe_gauge!("in_flight_requests", Unit::Count, "The number of domains currently being processed across all workers.");
        metrics::describe_counter!("rule_matches_total", Unit::Count, "The total number of times each rule has matched a domain.");
        metrics::describe_gauge!("rules_loaded_count", Unit::Count, "The current number of rules loaded into the rule engine.");
        metrics::describe_counter!("dns_queries_total", Unit::Count, "Total number of DNS queries performed, labeled by their outcome.");
        metrics::describe_histogram!("dns_resolution_duration_seconds", Unit::Seconds, "A histogram of the latency for DNS resolutions.");
        metrics::describe_histogram!("regex_match_duration_seconds", Unit::Seconds, "A histogram of the latency for regex matching operations.");
        metrics::describe_histogram!("rule_matching_duration_seconds", Unit::Seconds, "The time taken to match a domain against all rules.");
        metrics::describe_histogram!("processing_duration_seconds", Unit::Seconds, "A histogram of the total processing time from domain ingestion to rule completion.");
        metrics::describe_histogram!(
            "alert_build_duration_seconds",
            "The time taken to build an alert."
        );
        metrics::describe_histogram!(
            "alert_send_duration_seconds",
            Unit::Seconds,
            "The time in seconds it takes to send an alert to the output channel."
        );
        metrics::describe_gauge!("dns_resolver_health_status", Unit::Count, "Health status of each configured DNS resolver (1 for healthy, 0 for unhealthy).");
        metrics::describe_gauge!("process_cpu_usage_percent", Unit::Percent, "The percentage of CPU time the `certwatch` process is currently using.");
        metrics::describe_gauge!("process_memory_usage_bytes", Unit::Bytes, "The amount of physical memory (resident set size) the `certwatch` process is using, in bytes.");
        metrics::describe_gauge!("websocket_connection_status", Unit::Count, "The status of the websocket connection (1 for connected, 0 for disconnected).");
        metrics::describe_counter!("websocket_disconnects_total", Unit::Count, "Total number of times the websocket has disconnected.");
        metrics::describe_histogram!("worker_loop_iteration_duration_seconds", Unit::Seconds, "Duration of each worker loop iteration in seconds.");
        metrics::describe_histogram!("dns_worker_scheduling_delay_seconds", Unit::Seconds, "The time a domain spends waiting for a DNS worker to become available.");

        // Handles (for application use)
        Self {
            domains_ingested_total: metrics::counter!("domains_ingested_total"),
            domains_processed_total: metrics::counter!("domains_processed_total"),
            domains_ignored_total: metrics::counter!("domains_ignored_total"),
            deduplicated_alerts_total: metrics::counter!("deduplicated_alerts_total"),
            worker_loop_iteration_duration_seconds: metrics::histogram!("worker_loop_iteration_duration_seconds"),
            dns_worker_scheduling_delay_seconds: metrics::histogram!("dns_worker_scheduling_delay_seconds"),
        }
    }

    /// Creates a `Metrics` instance that performs no operations.
    /// Used when metrics are disabled in the configuration.
    pub fn disabled() -> Self {
        // Create dummy counters that do nothing.
        Self {
            domains_ingested_total: metrics::counter!("disabled"),
            domains_processed_total: metrics::counter!("disabled"),
            domains_ignored_total: metrics::counter!("disabled"),
            deduplicated_alerts_total: metrics::counter!("disabled"),
            worker_loop_iteration_duration_seconds: metrics::histogram!("disabled"),
            dns_worker_scheduling_delay_seconds: metrics::histogram!("disabled"),
        }
    }

    /// Increments the counter for a specific rule match.
    pub fn increment_rule_match(&self, rule_name: &str) {
        metrics::counter!("rule_matches_total", "rule" => rule_name.to_string()).increment(1);
    }

    /// Sets the gauge for the number of loaded rules.
    pub fn set_rules_loaded_count(&self, count: u64) {
        metrics::gauge!("rules_loaded_count").set(count as f64);
    }

    /// Sets the gauge for the websocket connection status.
    pub fn set_websocket_connection_status(&self, status: u8) {
        metrics::gauge!("websocket_connection_status").set(status as f64);
    }

    /// Increments the counter for websocket disconnects.
    pub fn increment_websocket_disconnects(&self) {
        metrics::counter!("websocket_disconnects_total").increment(1);
    }

    /// Increments the counter for DNS queries with a specific status.
    pub fn increment_dns_query(&self, status: &str) {
        metrics::counter!("dns_queries_total", "status" => status.to_string()).increment(1);
    }

    /// Creates a `Metrics` instance suitable for testing.
    ///
    /// This method initializes the metrics system with a no-op recorder,
    /// allowing tests to instantiate components that require a `Metrics`
    /// handle without needing a full metrics backend.
    pub fn new_for_test() -> Self {
        // In a test context, we don't need a real recorder.
        // The `metrics` crate's default recorder is a no-op.
        Self::new()
    }
}

/// Builder for the metrics system.
///
/// This builder is responsible for initializing the `PrometheusRecorder`,
/// spawning the `MetricsServer`, and creating the `Metrics` handle.
pub struct MetricsBuilder {
    config: MetricsConfig,
}

impl MetricsBuilder {
    /// Creates a new `MetricsBuilder` with the given configuration.
    pub fn new(config: MetricsConfig) -> Self {
        Self { config }
    }

    /// Initializes the metrics system and returns a `Metrics` handle and an
    /// optional `MetricsServer`.
    ///
    /// If metrics are disabled in the configuration, this method returns a
    /// disabled `Metrics` instance and `None` for the server.
    ///
    /// # Arguments
    ///
       /// * `shutdown_rx` - A watch channel receiver for graceful shutdown.
       pub fn build(
           self,
           shutdown_rx: watch::Receiver<()>,
       ) -> (Metrics, Option<(MetricsServer, SocketAddr)>) {
           if !self.config.enabled {
               return (Metrics::disabled(), None);
           }
   
           let recorder = PrometheusBuilder::new()
                .set_buckets_for_metric(
                    Matcher::Suffix("duration_seconds".to_string()),
                    &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
                )
                .unwrap()
                .build_recorder();
           let handle = recorder.handle();
   
           // Bind the listener before installing the recorder to ensure we can
           // report the address even if the recorder fails to install.
           let listener = match std::net::TcpListener::bind(self.config.listen_address) {
               Ok(listener) => listener,
               Err(e) => {
                   error!(
                       "Failed to bind metrics server to {}: {}",
                       self.config.listen_address, e
                   );
                   return (Metrics::disabled(), None);
               }
           };
   
           let addr = match listener.local_addr() {
               Ok(addr) => addr,
               Err(e) => {
                   error!("Failed to get local address for metrics server: {}", e);
                   return (Metrics::disabled(), None);
               }
           };
   
           // The listener must be non-blocking to be used with Tokio.
           listener.set_nonblocking(true).unwrap();
           let listener = TcpListener::from_std(listener).unwrap();
   
           if let Err(e) = metrics::set_global_recorder(recorder) {
               error!("Failed to install Prometheus recorder: {}", e);
               return (Metrics::disabled(), None);
           }
   
           let metrics = Metrics::new();
           let server = MetricsServer::new(listener, handle, shutdown_rx);
   
           if self.config.system_metrics_enabled {
               let system_collector = SystemCollector::new();
               tokio::spawn(system_collector.run());
           }
   
           (metrics, Some((server, addr)))
       }
   }

pub mod server;
pub mod system;