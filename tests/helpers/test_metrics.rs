#![allow(dead_code)]
//! A simple in-memory metrics recorder for testing.

use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, Unit};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Debug, Clone, Default)]
pub struct TestMetrics {
    counters: Arc<Mutex<HashMap<String, u64>>>,
    notifier: Arc<Notify>,
}

impl TestMetrics {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get_counter(&self, name: &str) -> u64 {
        self.counters
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or(0)
    }

    pub fn reset(&self) {
        let mut counters = self.counters.lock().unwrap();
        counters.clear();
    }

    pub async fn wait_for_counter(&self, name: &str, value: u64, timeout: std::time::Duration) {
        let wait_future = async {
            while self.get_counter(name) < value {
                self.notifier.notified().await;
            }
        };

        if tokio::time::timeout(timeout, wait_future).await.is_err() {
            panic!(
                "Timeout waiting for counter '{}' to reach '{}'. Current value: {}",
                name,
                value,
                self.get_counter(name)
            );
        }
    }
}

impl Recorder for TestMetrics {
    fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: metrics::SharedString) {}
    fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: metrics::SharedString) {}
    fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: metrics::SharedString) {}

    fn register_counter(&self, key: &Key, _metadata: &Metadata) -> Counter {
        Counter::from_arc(Arc::new(MetricCounter {
            name: key.name().to_string(),
            counters: self.counters.clone(),
            notifier: self.notifier.clone(),
        }))
    }

    fn register_gauge(&self, _key: &Key, _metadata: &Metadata) -> Gauge {
        // Not implemented for this test helper
        Gauge::noop()
    }

    fn register_histogram(&self, _key: &Key, _metadata: &Metadata) -> Histogram {
        // Not implemented for this test helper
        Histogram::noop()
    }
}

#[derive(Debug)]
struct MetricCounter {
    name: String,
    counters: Arc<Mutex<HashMap<String, u64>>>,
    notifier: Arc<Notify>,
}

impl metrics::CounterFn for MetricCounter {
    fn increment(&self, value: u64) {
        let mut counters = self.counters.lock().unwrap();
        *counters.entry(self.name.clone()).or_insert(0) += value;
        self.notifier.notify_waiters();
    }

    fn absolute(&self, _value: u64) {
        // Not implemented
    }
}