//! IP address enrichment services.
//!
//! This module provides traits and implementations for enriching IP addresses
//! with additional data, such as ASN and GeoIP information.

pub mod health;
pub mod tsv_lookup;

pub use tsv_lookup::TsvAsnLookup;

use crate::core::{EnrichmentInfo, EnrichmentProvider};
use anyhow::Result;
use async_trait::async_trait;
use std::net::IpAddr;

/// An `EnrichmentProvider` that does nothing.
#[derive(Debug, Clone)]
pub struct NoOpEnrichmentProvider;

#[async_trait]
impl EnrichmentProvider for NoOpEnrichmentProvider {
    async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo> {
        Ok(EnrichmentInfo { ip, asn_info: None })
    }
}

#[cfg(feature = "test-utils")]
pub mod fake;