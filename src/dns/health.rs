//! DNS Health Monitoring Service
//!
//! This module provides a stateful health monitor for the DNS resolution system.
//! It periodically checks a known domain to determine if the configured DNS
//! resolver is operational.

use crate::{config::DnsHealthConfig, core::DnsResolver, internal_metrics::Metrics, task_manager::TaskManager};
use anyhow::Result;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, error, info, warn};

/// Represents the health state of the DNS resolver.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum HealthState {
    Healthy,
    Unhealthy,
}

/// Monitors the health of the DNS resolution system via periodic checks.
pub struct DnsHealth {
    is_healthy: Arc<AtomicBool>,
    nameservers: Vec<SocketAddr>,
    _metrics: Arc<Metrics>,
}

impl DnsHealth {
    /// Performs a quick, one-off health check to ensure the configured DNS
    /// resolver is responsive before starting the main application.
    pub async fn startup_check(
        resolver: &dyn DnsResolver,
        config: &DnsHealthConfig,
    ) -> Result<()> {
        info!("Performing startup DNS health check...");
        let check_domain = &config.check_domain;
        debug!(domain = check_domain, "Performing startup DNS health check");

        match resolver.resolve(check_domain).await {
            Ok(_) => {
                info!("DNS health check passed.");
                Ok(())
            }
            Err(e) => {
                anyhow::bail!("DNS health check failed: Resolver could not resolve '{}': {}. Please check your DNS configuration.", check_domain, e)
            }
        }
    }

    /// Creates a new DNS health monitor and starts its background check task.
    pub fn new(
        config: DnsHealthConfig,
        resolver: Arc<dyn DnsResolver>,
        task_manager: &TaskManager,
        metrics: Arc<Metrics>,
        nameservers: Vec<SocketAddr>,
        initial_state: HealthState,
    ) -> Arc<Self> {
        let is_healthy = Arc::new(AtomicBool::new(initial_state == HealthState::Healthy));
        let monitor = Arc::new(Self {
            is_healthy: is_healthy.clone(),
            nameservers,
            _metrics: metrics,
        });

        // Set initial health for all nameservers to healthy
        monitor.update_health_gauge(true);

        if config.enabled {
            // Start the background health check task
            let monitor_clone = Arc::clone(&monitor);
            let mut shutdown_rx = task_manager.get_shutdown_rx();
            task_manager.spawn("DnsHealthCheck", async move {
                debug!("Spawning DNS periodic health check task.");
                let mut interval = tokio::time::interval(Duration::from_secs(config.interval_seconds));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.changed() => {
                            info!("DNS health monitor task received shutdown signal.");
                            break;
                        }
                        _ = interval.tick() => {
                            monitor_clone.perform_health_check(&*resolver, &config.check_domain).await;
                        }
                    }
                }
                info!("DNS health monitor task finished.");
            });
        }

        monitor
    }

    /// Returns `true` if the DNS resolver is currently considered healthy.
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::Relaxed)
    }


    /// Performs a single health check by resolving a domain.
    async fn perform_health_check(&self, resolver: &dyn DnsResolver, check_domain: &str) {
        let current_health = self.is_healthy();
        match resolver.resolve(check_domain).await {
            Ok(_) => {
                if !current_health {
                    self.is_healthy.store(true, Ordering::Relaxed);
                    self.update_health_gauge(true);
                    info!(domain = %check_domain, "DNS resolver has RECOVERED and is now HEALTHY.");
                }
            }
            Err(e) => {
                if current_health {
                    self.is_healthy.store(false, Ordering::Relaxed);
                    self.update_health_gauge(false);
                    error!(domain = %check_domain, error = %e, "DNS health check failed. Resolver is now UNHEALTHY.");
                } else {
                    warn!(domain = %check_domain, error = %e, "DNS health check failed. Resolver remains UNHEALTHY.");
                }
            }
        }
    }

    /// Updates the health state gauge metric.
    fn update_health_gauge(&self, is_healthy: bool) {
        let value = if is_healthy { 1.0 } else { 0.0 };
        for ns in &self.nameservers {
            metrics::gauge!("dns_resolver_health_status", "resolver_ip" => ns.to_string())
                .set(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dns::test_utils::FakeDnsResolver, internal_metrics::Metrics, task_manager::TaskManager};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::watch;

    #[tokio::test(start_paused = true)]
    async fn test_health_state_transitions() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let resolver = Arc::new(FakeDnsResolver::new());
        let config = DnsHealthConfig {
            enabled: true,
            interval_seconds: 5,
            check_domain: "google.com".to_string(),
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task_manager = TaskManager::new(shutdown_rx);

        // Add a success response for the initial health check that runs upon creation.
        resolver.add_success_response("google.com", Default::default());

        let monitor = DnsHealth::new(
            config.clone(),
            resolver.clone(),
            &task_manager,
            Arc::new(Metrics::new_for_test()),
            vec![],
            HealthState::Healthy,
        );

        // Let the initial check run.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(monitor.is_healthy(), "Should be healthy initially");

        // --- Transition to Unhealthy ---
        // Add a failure response for the next scheduled check.
        resolver.add_error_response("google.com", "Simulated NXDOMAIN");

        // Advance time to trigger the next health check.
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::time::sleep(Duration::from_millis(10)).await; // Allow background task to run.

        assert!(!monitor.is_healthy(), "Should become unhealthy after check fails");

        // --- Transition back to Healthy ---
        // Add a success response for the next scheduled check.
        resolver.add_success_response("google.com", Default::default());

        // Advance time for the next check.
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::time::sleep(Duration::from_millis(10)).await; // Allow background task to run.

        assert!(monitor.is_healthy(), "Should recover to healthy after check succeeds");

        // Ensure shutdown works.
        drop(shutdown_tx);
        task_manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_startup_check() {
        let resolver = FakeDnsResolver::new();
        let config = DnsHealthConfig {
            enabled: true,
            interval_seconds: 5,
            check_domain: "google.com".to_string(),
        };

        // Test success
        resolver.add_success_response("google.com", Default::default());
        let result = DnsHealth::startup_check(&resolver, &config).await;
        assert!(result.is_ok());

        // Test failure
        resolver.add_error_response("google.com", "Simulated failure");
        let result = DnsHealth::startup_check(&resolver, &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DNS health check failed"));
    }
}