pub mod health;
pub mod manager;
pub mod resolver;
#[cfg(feature = "test-utils")]
pub mod test_utils;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use health::DnsHealthMonitor;
pub use manager::{DnsResolutionManager, ResolvedNxDomain};
pub use resolver::HickoryDnsResolver;
pub use crate::core::DnsResolver;


#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DnsRetryConfig {
    /// Number of retries for standard failures (timeouts, server errors)
    pub standard_retries: u32,
    /// Initial backoff delay for standard failures in milliseconds
    pub standard_initial_backoff_ms: u64,
    /// Number of retries for NXDOMAIN responses
    pub nxdomain_retries: u32,
    /// Initial backoff delay for NXDOMAIN responses in milliseconds
    pub nxdomain_initial_backoff_ms: u64,
}

impl Default for DnsRetryConfig {
    fn default() -> Self {
        Self {
            standard_retries: 3,
            standard_initial_backoff_ms: 500,
            nxdomain_retries: 5,
            nxdomain_initial_backoff_ms: 10000,
        }
    }
}

#[derive(Error, Debug, Clone)]
pub enum DnsError {
    #[error("DNS resolution failed: {0}")]
    Resolution(String),

    #[error("DNS resolution cancelled due to shutdown")]
    Shutdown,
}