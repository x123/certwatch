//! # System Metrics Collector
//!
//! This module defines the `SystemCollector`, a component responsible for
//! gathering and reporting system-level metrics, such as CPU and memory usage.
//!
//! The `SystemCollector` runs in a dedicated background task, periodically
//! querying the system for resource utilization information via the `sysinfo`
//! crate and updating the corresponding Prometheus gauges. This provides a
//! near real-time view of the application's resource footprint.

use log::error;
use std::time::Duration;
use sysinfo::System;
use tokio::time;

const SYSTEM_METRICS_COLLECTION_INTERVAL: Duration = Duration::from_secs(10);

/// A collector for system-level metrics.
///
/// This struct encapsulates the logic for gathering CPU and memory usage
/// for the current process.
pub struct SystemCollector {
    system: System,
}

impl SystemCollector {
    /// Creates a new `SystemCollector`.
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Runs the system metrics collection loop.
    ///
    /// This method should be spawned as a background task. It will periodically
    /// refresh system information and update the relevant gauges.
    pub async fn run(mut self) {
        let mut interval = time::interval(SYSTEM_METRICS_COLLECTION_INTERVAL);
        let pid = match sysinfo::get_current_pid() {
            Ok(pid) => pid,
            Err(e) => {
                error!("Failed to get current PID: {}", e);
                return;
            }
        };

        loop {
            interval.tick().await;
            self.system.refresh_cpu();

            // `refresh_process` updates all information for a specific process,
            // including memory. It returns false if the process is not found.
            if self.system.refresh_process(pid) {
                if let Some(process) = self.system.process(pid) {
                    metrics::gauge!("process_cpu_usage_percent").set(process.cpu_usage() as f64);
                    // Per the metric description, we report the resident set size (physical memory).
                    metrics::gauge!("process_memory_usage_bytes")
                        .set(process.memory() as f64);
                }
            } else {
                error!(
                    "SystemCollector: Monitored process with PID {} no longer found. Collector is shutting down.",
                    pid
                );
                break;
            }
        }
    }
}