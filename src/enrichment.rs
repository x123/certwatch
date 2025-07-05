//! IP address enrichment services
//!
//! This module provides services for enriching IP addresses with ASN and GeoIP
//! data using MaxMind databases.

use crate::core::{AsnData, EnrichmentInfo, EnrichmentProvider, GeoIpInfo};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use maxminddb::{geoip2, MaxMindDBError, Reader};
use std::net::IpAddr;
use std::path::Path;

/// An enrichment provider that uses MaxMind DB files for ASN and GeoIP lookups.
#[derive(Debug)]
pub struct MaxmindEnrichmentProvider {
    asn_reader: Reader<Vec<u8>>,
    // In the future, a geoip_reader would go here.
    // For now, we'll keep the structure but won't use it.
}

impl MaxmindEnrichmentProvider {
    /// Creates a new enrichment service from MaxMind database files.
    ///
    /// # Arguments
    /// * `asn_db_path` - Path to the MaxMind GeoLite2-ASN.mmdb file.
    /// * `geoip_db_path` - Path to the MaxMind GeoLite2-Country.mmdb file.
    pub fn new<P: AsRef<Path>>(asn_db_path: P, _geoip_db_path: P) -> Result<Self> {
        let asn_reader = Reader::open_readfile(asn_db_path)
            .map_err(|e| anyhow!("Failed to open MaxMind ASN database: {}", e))?;
        
        // In the future, we would open the GeoIP database here.
        // let geoip_reader = Reader::open_readfile(geoip_db_path)
        //     .map_err(|e| anyhow!("Failed to open MaxMind GeoIP database: {}", e))?;

        Ok(Self { asn_reader })
    }
}

#[async_trait]
impl EnrichmentProvider for MaxmindEnrichmentProvider {
    async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo> {
        let mut info = EnrichmentInfo {
            ip,
            ..Default::default()
        };

        // Perform ASN lookup
        match self.asn_reader.lookup::<geoip2::Asn>(ip) {
            Ok(asn_data) => {
                if let Some(as_number) = asn_data.autonomous_system_number {
                    let as_name = asn_data
                        .autonomous_system_organization
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("AS{}", as_number));
                    info.asn = Some(AsnData {
                        as_number,
                        as_name,
                    });
                }
            }
            Err(e) => {
                if !matches!(e, MaxMindDBError::AddressNotFoundError(_)) {
                    log::warn!("ASN lookup failed for {}: {}", ip, e);
                }
            }
        }

        // Perform GeoIP lookup (placeholder)
        // In the future, this would use the geoip_reader.
        info.geoip = Some(GeoIpInfo {
            country_code: "XX".to_string(), // Placeholder until GeoIP DB is integrated
        });

        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::Ipv4Addr;
    use std::sync::Mutex;

    /// Fake enrichment provider for testing.
    #[derive(Default)]
    pub struct FakeEnrichmentProvider {
        responses: Mutex<HashMap<IpAddr, Result<EnrichmentInfo, String>>>,
    }

    impl FakeEnrichmentProvider {
        pub fn new() -> Self {
            Default::default()
        }

        /// Configure the provider to return a successful response for an IP.
        pub fn add_response(&self, ip: IpAddr, info: EnrichmentInfo) {
            let mut responses = self.responses.lock().unwrap();
            responses.insert(ip, Ok(info));
        }

        /// Configure the provider to return an error for an IP.
        pub fn add_error(&self, ip: IpAddr, error: &str) {
            let mut responses = self.responses.lock().unwrap();
            responses.insert(ip, Err(error.to_string()));
        }
    }

    #[async_trait]
    impl EnrichmentProvider for FakeEnrichmentProvider {
        async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo> {
            let responses = self.responses.lock().unwrap();
            match responses.get(&ip) {
                Some(Ok(info)) => Ok(info.clone()),
                Some(Err(error)) => Err(anyhow!("{}", error)),
                None => Ok(EnrichmentInfo {
                    ip,
                    ..Default::default()
                }), // Return empty info if not configured
            }
        }
    }

    #[tokio::test]
    async fn test_fake_enrichment_provider_success() {
        let provider = FakeEnrichmentProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        let expected_info = EnrichmentInfo {
            ip,
            asn: Some(AsnData {
                as_number: 15169,
                as_name: "Google LLC".to_string(),
            }),
            geoip: Some(GeoIpInfo {
                country_code: "US".to_string(),
            }),
        };

        provider.add_response(ip, expected_info.clone());

        let result = provider.enrich(ip).await.unwrap();
        assert_eq!(result.ip, expected_info.ip);
        assert_eq!(result.asn.unwrap().as_number, 15169);
        assert_eq!(result.geoip.unwrap().country_code, "US");
    }

    #[tokio::test]
    async fn test_fake_enrichment_provider_partial_data() {
        let provider = FakeEnrichmentProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let expected_info = EnrichmentInfo {
            ip,
            asn: Some(AsnData {
                as_number: 13335,
                as_name: "Cloudflare, Inc.".to_string(),
            }),
            geoip: None,
        };

        provider.add_response(ip, expected_info.clone());

        let result = provider.enrich(ip).await.unwrap();
        assert_eq!(result.ip, expected_info.ip);
        assert!(result.asn.is_some());
        assert!(result.geoip.is_none());
    }

    #[tokio::test]
    async fn test_fake_enrichment_provider_error() {
        let provider = FakeEnrichmentProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        provider.add_error(ip, "Critical database failure");

        let result = provider.enrich(ip).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Critical database failure"));
    }

    #[tokio::test]
    async fn test_fake_enrichment_provider_no_response_configured() {
        let provider = FakeEnrichmentProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let result = provider.enrich(ip).await.unwrap();
        assert_eq!(result.ip, ip);
        assert!(result.asn.is_none());
        assert!(result.geoip.is_none());
    }
}
