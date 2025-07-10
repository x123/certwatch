pub mod health;
pub mod manager;
pub mod resolver;
#[cfg(feature = "test-utils")]
pub mod test_utils;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use health::DnsHealth;
pub use manager::{DnsResolutionManager, ResolvedNxDomain};
pub use resolver::HickoryDnsResolver;
pub use crate::core::DnsResolver;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DnsRetryConfig {
    pub retries: Option<u32>,
    pub backoff_ms: Option<u64>,
    pub nxdomain_retries: Option<u32>,
    pub nxdomain_backoff_ms: Option<u64>,
}

impl Default for DnsRetryConfig {
    fn default() -> Self {
        Self {
            retries: Some(3),
            backoff_ms: Some(500),
            nxdomain_retries: Some(5),
            nxdomain_backoff_ms: Some(10000),
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