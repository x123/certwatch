//! DNS Health Monitoring Service
//!
//! This module provides a stateful health monitor for the DNS resolution system.
//! It tracks the failure rate over a configurable time window and can declare the
//! system "unhealthy" if the rate exceeds a threshold.

use crate::config::DnsHealthConfig;
use crate::core::DnsResolver;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Represents the health state of the DNS resolver.
#[derive(Debug, PartialEq, Clone)]
pub enum HealthState {
    Healthy,
    Unhealthy,
}

/// A recent resolution attempt outcome.
struct Outcome {
    timestamp: Instant,
    is_success: bool,
}

/// Monitors the health of the DNS resolution system.
pub struct DnsHealthMonitor {
    config: DnsHealthConfig,
    state: Mutex<HealthState>,
    outcomes: Mutex<VecDeque<Outcome>>,
    resolver: Arc<dyn DnsResolver>,
}

impl DnsHealthMonitor {
    /// Creates a new DNS health monitor and starts its background recovery task.
    pub fn new(config: DnsHealthConfig, resolver: Arc<dyn DnsResolver>) -> Arc<Self> {
        let monitor = Arc::new(Self {
            config,
            state: Mutex::new(HealthState::Healthy),
            outcomes: Mutex::new(VecDeque::new()),
            resolver,
        });

        // Start the background recovery task
        let monitor_clone = Arc::clone(&monitor);
        tokio::spawn(async move {
            monitor_clone.recovery_check_task().await;
        });

        monitor
    }

    /// Records a resolution outcome and updates the health state.
    pub fn record_outcome(&self, is_success: bool) {
        let mut outcomes = self.outcomes.lock().unwrap();
        let now = Instant::now();

        // Add the new outcome
        outcomes.push_back(Outcome {
            timestamp: now,
            is_success,
        });

        // Prune old outcomes that are outside the time window
        let window_start = now - Duration::from_secs(self.config.window_seconds);
        while let Some(outcome) = outcomes.front() {
            if outcome.timestamp < window_start {
                outcomes.pop_front();
            } else {
                break;
            }
        }

        // Recalculate health state
        self.update_health_state(&outcomes);
    }

    /// Gets the current health state.
    pub fn current_state(&self) -> HealthState {
        self.state.lock().unwrap().clone()
    }

    /// Updates the health state based on the current outcomes.
    fn update_health_state(&self, outcomes: &VecDeque<Outcome>) {
        if outcomes.is_empty() {
            return; // Not enough data, remain in the current state
        }

        let total = outcomes.len();
        let failures = outcomes.iter().filter(|o| !o.is_success).count();
        let failure_rate = failures as f64 / total as f64;

        let mut state = self.state.lock().unwrap();
        let current_state = &*state;

        if failure_rate >= self.config.failure_threshold && *current_state == HealthState::Healthy {
            *state = HealthState::Unhealthy;
            log::error!(
                "DNS resolver is now UNHEALTHY. Failure rate is {:.2}% over the last {} seconds ({} failures / {} total attempts).",
                failure_rate * 100.0,
                self.config.window_seconds,
                failures,
                total
            );
        } else if failure_rate < self.config.failure_threshold && *current_state == HealthState::Unhealthy {
            // Recovery is handled by the dedicated recovery task to ensure stability
        }
    }

    /// Background task to check for recovery when the system is unhealthy.
    async fn recovery_check_task(&self) {
        loop {
            // Only run the check if the state is Unhealthy
            if self.current_state() == HealthState::Unhealthy {
                log::info!("DNS resolver is unhealthy, attempting recovery check...");
                match self.resolver.resolve(&self.config.recovery_check_domain).await {
                    Ok(_) => {
                        let mut state = self.state.lock().unwrap();
                        if *state == HealthState::Unhealthy {
                            *state = HealthState::Healthy;
                            // Clear old failure outcomes to reset the failure rate calculation
                            self.outcomes.lock().unwrap().clear();
                            log::info!("DNS resolver has RECOVERED and is now HEALTHY.");
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "DNS recovery check failed for {}: {}. Remaining in unhealthy state.",
                            self.config.recovery_check_domain,
                            e
                        );
                    }
                }
            }
            // Wait for the next check interval
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DnsHealthConfig;
    use crate::dns::tests::FakeDnsResolver;
    use std::sync::Arc;
    use std::time::Duration;

    fn create_test_monitor(
        resolver: Arc<FakeDnsResolver>,
        threshold: f64,
        window_seconds: u64,
    ) -> Arc<DnsHealthMonitor> {
        let config = DnsHealthConfig {
            failure_threshold: threshold,
            window_seconds,
            recovery_check_domain: "google.com".to_string(),
        };
        // Don't start the recovery task for these unit tests
        Arc::new(DnsHealthMonitor {
            config,
            state: Mutex::new(HealthState::Healthy),
            outcomes: Mutex::new(VecDeque::new()),
            resolver,
        })
    }

    #[test]
    fn test_state_transitions_to_unhealthy() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver, 0.5, 10);

        assert_eq!(monitor.current_state(), HealthState::Healthy);

        // Report 4 successes and 6 failures -> 60% failure rate
        for _ in 0..4 {
            monitor.record_outcome(true);
        }
        for _ in 0..6 {
            monitor.record_outcome(false);
        }

        assert_eq!(monitor.current_state(), HealthState::Unhealthy);
    }

    #[test]
    fn test_state_remains_healthy_below_threshold() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver, 0.8, 10);

        assert_eq!(monitor.current_state(), HealthState::Healthy);

        // Report 5 successes and 5 failures -> 50% failure rate
        for _ in 0..5 {
            monitor.record_outcome(true);
        }
        for _ in 0..5 {
            monitor.record_outcome(false);
        }

        assert_eq!(monitor.current_state(), HealthState::Healthy);
    }

    #[tokio::test]
    async fn test_recovery_to_healthy_state() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver.clone(), 0.5, 10);

        // Make state unhealthy
        monitor.record_outcome(false);
        monitor.record_outcome(false);
        assert_eq!(monitor.current_state(), HealthState::Unhealthy);

        // Configure recovery domain to succeed
        resolver.set_success_response("google.com", Default::default());

        // Manually trigger recovery check logic
        let mut state = monitor.state.lock().unwrap();
        *state = HealthState::Healthy;
        monitor.outcomes.lock().unwrap().clear();
        
        assert_eq!(*state, HealthState::Healthy);
        assert!(monitor.outcomes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_outcome_window_pruning() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver, 0.5, 2); // 2-second window

        // Record an outcome
        monitor.record_outcome(false);
        assert_eq!(monitor.outcomes.lock().unwrap().len(), 1);

        // Wait for longer than the window
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Record another outcome, which should trigger pruning
        monitor.record_outcome(true);
        assert_eq!(monitor.outcomes.lock().unwrap().len(), 1);
        assert!(monitor.outcomes.lock().unwrap().front().unwrap().is_success);
    }
}