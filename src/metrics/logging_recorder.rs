//! A metrics recorder that periodically logs all captured metrics.

use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, Unit, SharedString};
use metrics_util::registry::{AtomicStorage, Registry};
use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::Ordering;

/// A metrics recorder that periodically logs all captured metrics to `log::info!`.
pub struct LoggingRecorder {
    registry: Arc<Registry<Key, AtomicStorage>>,
}

impl LoggingRecorder {
    /// Creates a new `LoggingRecorder` and starts a background task to log metrics.
    ///
    /// # Arguments
    /// * `interval` - The interval at which to log the metrics.
    pub fn new(interval: Duration) -> Self {
        let registry = Arc::new(Registry::new(AtomicStorage));
        let recorder = Self {
            registry: registry.clone(),
        };

        // Spawn a background task to log metrics periodically
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                log::debug!("--- Metrics Snapshot ---");

                for (key, counter) in registry.get_counter_handles() {
                    let value = counter.load(Ordering::Relaxed);
                    if key.name().starts_with("agg.") {
                        // Aggregated metrics are logged at DEBUG level and reset.
                        if value > 0 {
                            let key_name = key.name();
                            let message = if key_name == "agg.domains_sent_to_output" {
                                format!("Sent {} domains to output channel in the last {}s", value, interval.as_secs())
                            } else if key_name == "agg.alerts_sent" {
                                format!("Sent {} alerts in the last {}s", value, interval.as_secs())
                            } else {
                                format!("[Agg. Counter] {}: {}", key, value)
                            };
                            log::debug!("{}", message);
                            counter.store(0, Ordering::Relaxed);
                        }
                    } else {
                        // Standard metrics are logged at INFO level.
                        log::info!("[Counter] {}: {}", key, value);
                    }
                }

                for (key, gauge) in registry.get_gauge_handles() {
                    let value = f64::from_bits(gauge.load(Ordering::Relaxed));
                    log::info!("[Gauge] {}: {}", key, value as u64);
                }
                // Note: Histograms are not logged in this simple implementation
            }
        });

        recorder
    }
}

impl Recorder for LoggingRecorder {
    fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        // Not implemented for this simple recorder
    }

    fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        // Not implemented for this simple recorder
    }

    fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        // Not implemented for this simple recorder
    }

    fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
        self.registry.get_or_create_counter(key, |c| c.clone()).into()
    }

    fn register_gauge(&self, key: &Key, _metadata: &Metadata<'_>) -> Gauge {
        self.registry.get_or_create_gauge(key, |g| g.clone()).into()
    }

    fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
        self.registry.get_or_create_histogram(key, |h| h.clone()).into()
    }
}