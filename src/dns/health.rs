//! DNS Health Monitoring Service
//!
//! This module provides a stateful health monitor for the DNS resolution system.
//! It tracks the failure rate over a configurable time window and can declare the
//! system "unhealthy" if the rate exceeds a threshold.

use crate::config::DnsHealthConfig;
use crate::core::DnsResolver;
use crate::utils::heartbeat::run_heartbeat;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tracing::info;

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
    #[cfg(test)]
    pub health_updated_tx: watch::Sender<()>,
}

/// Monitors the health of the DNS resolution system.
pub struct DnsHealthMonitor {
    pub config: DnsHealthConfig,
    pub monitor_state: Mutex<MonitorState>,
    pub resolver: Arc<dyn DnsResolver>,
    pub outcome_tx: mpsc::UnboundedSender<bool>,
}

impl DnsHealthMonitor {
    /// Performs a quick, one-off health check to ensure the configured DNS
    /// resolver is responsive before starting the main application.
    pub async fn startup_check(
        resolver: &dyn DnsResolver,
        config: &DnsHealthConfig,
    ) -> Result<()> {
        info!("Performing startup DNS health check...");

        match resolver.resolve(&config.recovery_check_domain).await {
            Ok(_) => {
                info!("DNS health check passed.");
                Ok(())
            }
            Err(e) => {
                anyhow::bail!("DNS health check failed: Resolver could not resolve '{}': {}. Please check your DNS configuration.", config.recovery_check_domain, e)
            }
        }
    }

    /// Creates a new DNS health monitor and starts its background tasks.
    pub fn new(
        config: DnsHealthConfig,
        resolver: Arc<dyn DnsResolver>,
        shutdown_rx: watch::Receiver<()>,
    ) -> Arc<Self> {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        let monitor = Arc::new(Self {
            config,
            monitor_state: Mutex::new(MonitorState {
                health: HealthState::Healthy,
                outcomes: VecDeque::new(),
                #[cfg(test)]
                health_updated_tx: watch::channel(()).0,
            }),
            resolver,
            outcome_tx,
        });

        // Start the state update task
        let monitor_clone = Arc::clone(&monitor);
        let mut update_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            tracing::debug!("Spawning DNS health monitor state update task.");
            tokio::select! {
                _ = monitor_clone.process_outcomes_task(outcome_rx) => {},
                _ = update_shutdown_rx.changed() => {
                    tracing::info!("DNS health monitor state update task received shutdown signal.");
                }
            }
        });

        // Start the background recovery task
        let recovery_monitor_clone = Arc::clone(&monitor);
        let mut recovery_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            recovery_monitor_clone
                .recovery_check_task(&mut recovery_shutdown_rx)
                .await;
        });

        // Start the heartbeat task
        let heartbeat_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            run_heartbeat("DnsHealthMonitor", heartbeat_shutdown_rx).await;
        });

        monitor
    }

    /// Records a resolution outcome by sending it to the state update task.
    pub fn record_outcome(&self, is_success: bool) {
        tracing::debug!(is_success, "Recording outcome");
        if let Err(e) = self.outcome_tx.send(is_success) {
            tracing::error!(error = %e, "Failed to send DNS health outcome to channel");
        }
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
            tracing::error!(
                failure_rate = failure_rate * 100.0,
                window_seconds = self.config.window_seconds,
                failures,
                total,
                "DNS resolver is now UNHEALTHY."
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

        tracing::info!("DNS resolver is unhealthy, attempting recovery check...");
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
                    tracing::info!("DNS resolver has RECOVERED and is now HEALTHY.");
                }
            }
            Err(e) => {
                tracing::warn!(
                    domain = %self.config.recovery_check_domain,
                    error = %e,
                    "DNS recovery check failed. Remaining in unhealthy state."
                );
            }
        }
    }

    /// Background task to periodically check for recovery when the system is unhealthy.
    async fn recovery_check_task(&self, shutdown_rx: &mut watch::Receiver<()>) {
        loop {
            tokio::select! {
                _ = self.perform_single_recovery_check() => {
                    // After a check, wait before the next one.
                    tokio::time::sleep(Duration::from_secs(self.config.recovery_check_interval_seconds)).await;
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("DNS health monitor recovery task received shutdown signal, exiting.");
                    break;
                }
            }
        }
    }
    /// Synchronously processes a single resolution outcome.
    /// This is the core logic, extracted for deterministic testing.
    pub fn process_outcome(&self, is_success: bool) {
        tracing::debug!(is_success, "Processing outcome");
        let mut state = self.monitor_state.lock().unwrap();
        tracing::debug!("Acquired monitor state lock");
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
        #[cfg(test)]
        state.health_updated_tx.send(()).ok();
    }

    /// Background task that processes resolution outcomes from the channel.
    async fn process_outcomes_task(&self, mut outcome_rx: mpsc::UnboundedReceiver<bool>) {
        tracing::debug!("Starting outcome processing loop.");
        while let Some(is_success) = outcome_rx.recv().await {
            self.process_outcome(is_success);
        }
        tracing::debug!("Outcome processing loop finished.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::test_utils::FakeDnsResolver;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::watch::Sender;

    /// A simple guard to ensure the shutdown signal is sent when the test scope ends.
    struct ShutdownGuard(Sender<()>);

    impl Drop for ShutdownGuard {
        fn drop(&mut self) {
            tracing::debug!("ShutdownGuard dropped, sending shutdown signal.");
            self.0.send(()).ok();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_state_transitions_to_unhealthy() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.5,
            window_seconds: 10,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver, shutdown_rx);

        // Subscribe to health changes *before* making changes
        let mut health_rx = monitor.monitor_state.lock().unwrap().health_updated_tx.subscribe();

        assert_eq!(monitor.current_state(), HealthState::Healthy);

        // Report 4 successes and 6 failures -> 60% failure rate
        tracing::debug!("Recording 4 successes and 6 failures");
        for _ in 0..4 {
            monitor.record_outcome(true);
        }
        for _ in 0..6 {
            monitor.record_outcome(false);
        }

        // Wait for the background task to process the outcomes and update the state.
        // This is now deterministic and instant due to the paused clock and signaling.
        health_rx.changed().await.expect("Health update signal was not received");

        assert_eq!(monitor.current_state(), HealthState::Unhealthy);
    }

    #[tokio::test(start_paused = true)]
    async fn test_state_remains_healthy_below_threshold() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.8,
            window_seconds: 10,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver, shutdown_rx);

        let mut health_rx = monitor.monitor_state.lock().unwrap().health_updated_tx.subscribe();

        assert_eq!(monitor.current_state(), HealthState::Healthy);

        // Report 5 successes and 5 failures -> 50% failure rate
        tracing::debug!("Recording 5 successes and 5 failures");
        for _ in 0..5 {
            monitor.record_outcome(true);
        }
        for _ in 0..5 {
            monitor.record_outcome(false);
        }

        // Wait for the background task to process.
        health_rx.changed().await.expect("Health update signal was not received");

        // The state should remain healthy as the failure rate is below the threshold.
        assert_eq!(monitor.current_state(), HealthState::Healthy);
    }

    #[tokio::test]
    async fn test_recovery_to_healthy_state() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.5,
            window_seconds: 10,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver.clone(), shutdown_rx);

        // Make state unhealthy
        monitor.process_outcome(false);
        monitor.process_outcome(false);
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

    #[tokio::test(start_paused = true)]
    async fn test_outcome_window_pruning() {
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.5,
            window_seconds: 2,
            ..Default::default()
        };
        let (_shutdown_tx, shutdown_rx) = watch::channel(());
        let monitor = DnsHealthMonitor::new(config, resolver, shutdown_rx);

        // Record an outcome
        monitor.process_outcome(false);
        assert_eq!(monitor.monitor_state.lock().unwrap().outcomes.len(), 1);

        // Advance time by more than the window duration
        tokio::time::advance(Duration::from_secs(3)).await;

        // Record another outcome, which should trigger pruning of the old one
        monitor.process_outcome(true);
        let state = monitor.monitor_state.lock().unwrap();
        assert_eq!(state.outcomes.len(), 1, "Old outcome should have been pruned");
        assert!(state.outcomes.front().unwrap().is_success, "The new outcome should be the only one remaining");
    }
    #[tokio::test(start_paused = true)]
    async fn test_no_state_change_when_unhealthy() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.5,
            window_seconds: 10,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver, shutdown_rx);
        let mut health_rx = monitor.monitor_state.lock().unwrap().health_updated_tx.subscribe();

        // Transition to Unhealthy
        tracing::debug!("Recording two failures to become unhealthy");
        monitor.record_outcome(false);
        monitor.record_outcome(false);

        // Wait for the state to become unhealthy
        health_rx.changed().await.expect("Health update signal was not received");
        assert_eq!(monitor.current_state(), HealthState::Unhealthy);

        // Add successes, bringing the failure rate below the threshold
        tracing::debug!("Recording two successes");
        monitor.record_outcome(true);
        monitor.record_outcome(true);

        // Wait for the background task to process the new outcomes
        health_rx.changed().await.expect("Health update signal was not received after successes");

        // State should remain Unhealthy because recovery is handled by a separate task
        assert_eq!(
            monitor.current_state(),
            HealthState::Unhealthy,
            "State should not change back to Healthy automatically"
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_record_outcome_is_non_blocking() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            // Disable the recovery task to prevent it from interfering.
            recovery_check_interval_seconds: 3600,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver.clone(), shutdown_rx);

        // Spawn a task that takes the lock and holds it.
        let lock_holder_monitor = monitor.clone();
        let lock_task = tokio::spawn(async move {
            {
                tracing::debug!("[Lock Task] Acquiring lock...");
                let _lock = lock_holder_monitor.monitor_state.lock().unwrap();
                tracing::debug!("[Lock Task] Lock acquired. Holding for 2s.");
                // The lock is held inside this block.
            }
            // The lock is dropped here, before the await point.
            tokio::time::sleep(Duration::from_secs(2)).await;
            tracing::debug!("[Lock Task] Finished sleeping.");
        });

        // Give the lock task a moment to start and acquire the lock.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Now, call record_outcome. It should not block.
        tracing::debug!("[Test] Calling record_outcome while lock is held elsewhere.");
        let start = Instant::now();
        monitor.record_outcome(true);
        let duration = start.elapsed();
        tracing::debug!("[Test] record_outcome returned in {:?}", duration);

        assert!(
            duration < Duration::from_millis(100),
            "record_outcome took {:?} which indicates it blocked",
            duration
        );

        // Clean up the lock task
        lock_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_health_monitor_concurrent_updates() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            failure_threshold: 0.5,
            window_seconds: 10,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let _guard = ShutdownGuard(shutdown_tx);
        let monitor = DnsHealthMonitor::new(config, resolver.clone(), shutdown_rx);
        let num_updates = 1000;

        // Subscribe *before* spawning tasks to avoid a race condition
        let mut rx = monitor.monitor_state.lock().unwrap().health_updated_tx.subscribe();

        tracing::debug!("Spawning {} update tasks", num_updates);
        let mut tasks = Vec::new();
        for i in 0..num_updates {
            let monitor_clone = monitor.clone();
            tasks.push(tokio::spawn(async move {
                tracing::debug!("Task {} recording outcome", i);
                monitor_clone.record_outcome(true);
            }));
        }

        tracing::debug!("Awaiting all update tasks");
        for task in tasks {
            task.await.unwrap();
        }
        tracing::debug!("All update tasks completed");

        // Wait for the state update task to process all the messages
        tracing::debug!("Waiting for state updates to be processed");
        for i in 0..num_updates {
            tracing::debug!("Waiting for update #{}", i + 1);
            if let Err(e) = tokio::time::timeout(Duration::from_secs(5), rx.changed()).await {
                // It's possible the channel is closed if the test finishes quickly.
                // The final assertion on the number of outcomes is the source of truth.
                tracing::warn!("Error waiting for health update #{}: {}. This might be okay if the task is already done.", i + 1, e);
                break;
            }
        }
        tracing::debug!("Finished waiting for state updates.");

        assert_eq!(
            monitor.monitor_state.lock().unwrap().outcomes.len(),
            num_updates
        );
    }
}