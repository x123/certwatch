//! DNS Health Monitoring Service
//!
//! This module provides a stateful health monitor for the DNS resolution system.
//! It tracks the failure rate over a configurable time window and can declare the
//! system "unhealthy" if the rate exceeds a threshold.

use crate::config::DnsHealthConfig;
use crate::core::DnsResolver;
use crate::utils::heartbeat::run_heartbeat;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Represents the health state of the DNS resolver.
#[derive(Debug, PartialEq, Clone)]
pub enum HealthState {
    Healthy,
    Unhealthy,
}

/// A recent resolution attempt outcome.
pub struct Outcome {
    pub timestamp: Instant,
    pub is_success: bool,
}

/// Holds the mutable state of the health monitor, protected by a single Mutex.
pub struct MonitorState {
    pub health: HealthState,
    pub outcomes: VecDeque<Outcome>,
}

/// Monitors the health of the DNS resolution system.
pub struct DnsHealthMonitor {
    pub config: DnsHealthConfig,
    pub monitor_state: Mutex<MonitorState>,
    pub resolver: Arc<dyn DnsResolver>,
}

impl DnsHealthMonitor {
    /// Creates a new DNS health monitor and starts its background recovery task.
    pub fn new(
        config: DnsHealthConfig,
        resolver: Arc<dyn DnsResolver>,
        shutdown_rx: watch::Receiver<()>,
    ) -> Arc<Self> {
        let monitor = Arc::new(Self {
            config,
            monitor_state: Mutex::new(MonitorState {
                health: HealthState::Healthy,
                outcomes: VecDeque::new(),
            }),
            resolver,
        });

        // Start the background recovery task
        let monitor_clone = Arc::clone(&monitor);
        let mut recovery_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = monitor_clone.recovery_check_task() => {},
                _ = recovery_shutdown_rx.changed() => {
                    log::info!("DNS health monitor received shutdown signal.");
                }
            }
        });

        // Start the heartbeat task
        let heartbeat_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            run_heartbeat("DnsHealthMonitor", heartbeat_shutdown_rx).await;
        });

        monitor
    }

    /// Records a resolution outcome and updates the health state.
    pub fn record_outcome(&self, is_success: bool) {
        let mut state = self.monitor_state.lock().unwrap();
        let now = Instant::now();

        // Add the new outcome
        state.outcomes.push_back(Outcome {
            timestamp: now,
            is_success,
        });

        // Prune old outcomes that are outside the time window
        let window_start = now - Duration::from_secs(self.config.window_seconds);
        while let Some(outcome) = state.outcomes.front() {
            if outcome.timestamp < window_start {
                state.outcomes.pop_front();
            } else {
                break;
            }
        }

        // Recalculate health state
        self.update_health_state(&mut state);
    }

    /// Gets the current health state.
    pub fn current_state(&self) -> HealthState {
        self.monitor_state.lock().unwrap().health.clone()
    }

    /// Updates the health state based on the current outcomes.
    fn update_health_state(&self, state: &mut MonitorState) {
        if state.outcomes.is_empty() {
            return; // Not enough data, remain in the current state
        }

        let total = state.outcomes.len();
        let failures = state.outcomes.iter().filter(|o| !o.is_success).count();
        let failure_rate = failures as f64 / total as f64;

        if failure_rate >= self.config.failure_threshold && state.health == HealthState::Healthy {
            state.health = HealthState::Unhealthy;
            log::error!(
                "DNS resolver is now UNHEALTHY. Failure rate is {:.2}% over the last {} seconds ({} failures / {} total attempts).",
                failure_rate * 100.0,
                self.config.window_seconds,
                failures,
                total
            );
        } else if failure_rate < self.config.failure_threshold && state.health == HealthState::Unhealthy {
            // Recovery is handled by the dedicated recovery task to ensure stability
        }
    }

    /// Performs a single recovery check.
    /// If the check is successful, it atomically updates the state to Healthy and
    /// clears the outcome history.
    async fn perform_single_recovery_check(&self) {
        if self.current_state() != HealthState::Unhealthy {
            return;
        }

        log::info!("DNS resolver is unhealthy, attempting recovery check...");
        match self.resolver.resolve(&self.config.recovery_check_domain).await {
            Ok(_) => {
                // SAFETY: The lock is held for the entire state transition to ensure
                // that the health status is updated and the outcomes are cleared
                // in a single atomic operation. This prevents a race condition where
                // a new failure could be recorded after the state is set to Healthy
                // but before the outcomes are cleared.
                let mut state = self.monitor_state.lock().unwrap();
                if state.health == HealthState::Unhealthy {
                    state.health = HealthState::Healthy;
                    // Clear old failure outcomes to reset the failure rate calculation
                    state.outcomes.clear();
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

    /// Background task to periodically check for recovery when the system is unhealthy.
    async fn recovery_check_task(&self) {
        loop {
            self.perform_single_recovery_check().await;
            // Wait for the next check interval
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::tests::{create_test_monitor, FakeDnsResolver};
    use std::sync::Arc;
    use std::time::Duration;

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
        let monitor = Arc::new(create_test_monitor(resolver.clone(), 0.5, 10));

        // Make state unhealthy
        monitor.record_outcome(false);
        monitor.record_outcome(false);
        assert_eq!(monitor.current_state(), HealthState::Unhealthy);
        assert_eq!(monitor.monitor_state.lock().unwrap().outcomes.len(), 2);

        // Configure recovery domain to succeed
        resolver.add_success_response("google.com", Default::default());

        // Manually trigger a single recovery check, simulating the background task's action
        monitor.perform_single_recovery_check().await;

        // State should be healthy and outcomes cleared
        let state = monitor.monitor_state.lock().unwrap();
        assert_eq!(state.health, HealthState::Healthy);
        assert!(state.outcomes.is_empty());
    }

    #[tokio::test]
    async fn test_outcome_window_pruning() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver, 0.5, 2); // 2-second window

        // Record an outcome
        monitor.record_outcome(false);
        assert_eq!(monitor.monitor_state.lock().unwrap().outcomes.len(), 1);

        // Wait for longer than the window
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Record another outcome, which should trigger pruning
        monitor.record_outcome(true);
        let state = monitor.monitor_state.lock().unwrap();
        assert_eq!(state.outcomes.len(), 1);
        assert!(state.outcomes.front().unwrap().is_success);
    }
    #[test]
    fn test_no_state_change_when_unhealthy() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let monitor = create_test_monitor(resolver, 0.5, 10);

        // Transition to Unhealthy
        monitor.record_outcome(false);
        monitor.record_outcome(false);
        assert_eq!(monitor.current_state(), HealthState::Unhealthy);

        // Add successes, bringing the failure rate below the threshold (2 failures, 2 successes -> 50%)
        monitor.record_outcome(true);
        monitor.record_outcome(true);

        // State should remain Unhealthy because recovery is handled by a separate task
        assert_eq!(
            monitor.current_state(),
            HealthState::Unhealthy,
            "State should not change back to Healthy automatically"
        );
    }
}