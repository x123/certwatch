//! A metrics recorder that periodically logs all captured metrics.

use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, Unit, SharedString};
use crate::utils::heartbeat::run_heartbeat;
use metrics_util::registry::{AtomicStorage, Registry};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A metrics recorder that periodically logs all captured metrics to `log::info!`.
pub struct LoggingRecorder {
    registry: Arc<Registry<Key, AtomicStorage>>,
}

impl LoggingRecorder {
    /// Creates a new `LoggingRecorder` and starts a background task to log metrics.
    ///
    /// # Arguments
    /// * `aggregation_interval` - The interval at which to log the metrics.
    pub fn new(
        aggregation_interval: Duration,
        shutdown_rx: watch::Receiver<()>,
    ) -> (Self, JoinHandle<()>) {
        let registry = Arc::new(Registry::new(AtomicStorage));
        let recorder = Self {
            registry: registry.clone(),
        };

        // Spawn a background task to log metrics periodically
        let mut logging_shutdown_rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(aggregation_interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracing::debug!("--- Metrics Snapshot ---");

                        for (key, counter) in registry.get_counter_handles() {
                            let value = counter.load(Ordering::Relaxed);
                            if key.name().starts_with("agg.") {
                                // Aggregated metrics are logged at DEBUG level and reset.
                                if value > 0 {
                                    let key_name = key.name();
                                    let message = if key_name == "agg.domains_sent_to_output" {
                                        format!("Sent {} domains to output channel in the last {}s", value, aggregation_interval.as_secs())
                                    } else if key_name == "agg.alerts_sent" {
                                        format!("Sent {} alerts in the last {}s", value, aggregation_interval.as_secs())
                                    } else {
                                        format!("[Agg. Counter] {}: {}", key, value)
                                    };
                                    tracing::debug!("{}", message);
                                    counter.store(0, Ordering::Relaxed);
                                }
                            } else {
                                // Standard metrics are logged at INFO level.
                                tracing::info!("[Counter] {}: {}", key, value);
                            }
                        }

                        for (key, gauge) in registry.get_gauge_handles() {
                            let value = f64::from_bits(gauge.load(Ordering::Relaxed));
                            tracing::info!("[Gauge] {}: {}", key, value as u64);
                        }
                        // Note: Histograms are not logged in this simple implementation
                    }
                    _ = logging_shutdown_rx.changed() => {
                        tracing::info!("Metrics logging task received shutdown signal.");
                        break;
                    }
                }
            }
        });

        // Spawn the heartbeat task
        tokio::spawn(async move {
            run_heartbeat("LoggingRecorder", shutdown_rx).await;
        });

        (recorder, handle)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use metrics::{Key, Recorder};

    #[tokio::test]
    async fn test_aggregation_resets_counter() {
        // 1. Create a recorder with a short interval.
        let interval = Duration::from_millis(50);
        // Keep the sender alive so the background task doesn't exit immediately.
        let (tx, rx) = watch::channel(());
        let (recorder, handle) = LoggingRecorder::new(interval, rx);
        let registry = recorder.registry.clone();

        // 2. Register a counter.
        let counter_key = Key::from_name("agg.test_counter");
        let counter_handle = recorder.register_counter(
            &counter_key,
            &metrics::Metadata::new("test", metrics::Level::INFO, Some("test")),
        );

        // 3. Increment the counter and check its value.
        counter_handle.increment(5);
        let initial_value = registry
            .get_counter_handles()
            .get(&counter_key)
            .unwrap()
            .load(Ordering::Relaxed);
        assert_eq!(
            initial_value, 5,
            "Counter should have the initial incremented value"
        );

        // 4. Wait for the aggregation task to run.
        tokio::time::sleep(interval + Duration::from_millis(20)).await;

        // 5. Check if the counter was reset.
        let value_after_reset = registry
            .get_counter_handles()
            .get(&counter_key)
            .unwrap()
            .load(Ordering::Relaxed);
        assert_eq!(
            value_after_reset, 0,
            "Aggregated counter should be reset to 0 after the interval"
        );

        // 6. Cleanly shut down the task to avoid test warnings.
        drop(tx);
        let _ = handle.await;
    }
}