//! ASN enrichment service for IP address geolocation
//!
//! This module provides ASN (Autonomous System Number) lookup functionality
//! using MaxMind GeoLite2-ASN database for IP address enrichment.

use crate::core::{AsnInfo, AsnLookup};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use maxminddb::{geoip2, MaxMindDBError, Reader};
use std::net::IpAddr;
use std::path::Path;

/// ASN lookup implementation using MaxMind GeoLite2-ASN database
#[derive(Debug)]
pub struct MaxmindAsnLookup {
    reader: Reader<Vec<u8>>,
}

impl MaxmindAsnLookup {
    /// Creates a new ASN lookup service from a MaxMind database file
    ///
    /// # Arguments
    /// * `db_path` - Path to the MaxMind GeoLite2-ASN.mmdb file
    ///
    /// # Returns
    /// * `Ok(MaxmindAsnLookup)` if the database was loaded successfully
    /// * `Err` if the database file could not be read or is invalid
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let reader = Reader::open_readfile(db_path)
            .map_err(|e| anyhow!("Failed to open MaxMind database: {}", e))?;
        
        Ok(Self { reader })
    }

    /// Creates a new ASN lookup service from raw database bytes
    ///
    /// This is useful for testing with embedded test databases
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let reader = Reader::from_source(bytes)
            .map_err(|e| anyhow!("Failed to create MaxMind reader from bytes: {}", e))?;
        
        Ok(Self { reader })
    }
}

#[async_trait]
impl AsnLookup for MaxmindAsnLookup {
    async fn lookup(&self, ip: IpAddr) -> Result<AsnInfo> {
        // Perform the ASN lookup
        let asn_data: geoip2::Asn = self.reader
            .lookup(ip)
            .map_err(|e| match e {
                MaxMindDBError::AddressNotFoundError(_) => {
                    anyhow!("IP address {} not found in ASN database", ip)
                }
                _ => anyhow!("ASN lookup failed for {}: {}", ip, e),
            })?;

        // Extract ASN information
        let as_number = asn_data.autonomous_system_number
            .ok_or_else(|| anyhow!("No ASN number found for IP {}", ip))?;

        let as_name = asn_data.autonomous_system_organization
            .ok_or_else(|| anyhow!("No ASN organization found for IP {}", ip))?
            .to_string();

        // For the country code, we need to do a separate GeoIP lookup
        // Since we only have the ASN database, we'll use a placeholder
        // In a real implementation, you'd want to use GeoLite2-Country.mmdb as well
        let country_code = "XX".to_string(); // Placeholder

        Ok(AsnInfo {
            ip,
            as_number,
            as_name,
            country_code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// Fake ASN lookup for testing
    pub struct FakeAsnLookup {
        responses: std::collections::HashMap<IpAddr, Result<AsnInfo, String>>,
    }

    impl FakeAsnLookup {
        pub fn new() -> Self {
            Self {
                responses: std::collections::HashMap::new(),
            }
        }

        /// Configure the lookup to return a successful response for an IP
        pub fn set_success_response(&mut self, ip: IpAddr, asn_info: AsnInfo) {
            self.responses.insert(ip, Ok(asn_info));
        }

        /// Configure the lookup to return an error for an IP
        pub fn set_error_response(&mut self, ip: IpAddr, error: &str) {
            self.responses.insert(ip, Err(error.to_string()));
        }
    }

    #[async_trait]
    impl AsnLookup for FakeAsnLookup {
        async fn lookup(&self, ip: IpAddr) -> Result<AsnInfo> {
            match self.responses.get(&ip) {
                Some(Ok(asn_info)) => Ok(asn_info.clone()),
                Some(Err(error)) => Err(anyhow!("{}", error)),
                None => Err(anyhow!("No response configured for IP {}", ip)),
            }
        }
    }

    #[tokio::test]
    async fn test_fake_asn_lookup_success() {
        let mut lookup = FakeAsnLookup::new();
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        let expected_asn = AsnInfo {
            ip,
            as_number: 15169,
            as_name: "Google LLC".to_string(),
            country_code: "US".to_string(),
        };

        lookup.set_success_response(ip, expected_asn.clone());

        let result = lookup.lookup(ip).await.unwrap();
        assert_eq!(result.ip, expected_asn.ip);
        assert_eq!(result.as_number, expected_asn.as_number);
        assert_eq!(result.as_name, expected_asn.as_name);
        assert_eq!(result.country_code, expected_asn.country_code);
    }

    #[tokio::test]
    async fn test_fake_asn_lookup_error() {
        let mut lookup = FakeAsnLookup::new();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        
        lookup.set_error_response(ip, "Private IP address not in database");

        let result = lookup.lookup(ip).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Private IP address not in database"));
    }

    #[tokio::test]
    async fn test_fake_asn_lookup_no_response_configured() {
        let lookup = FakeAsnLookup::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        let result = lookup.lookup(ip).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No response configured"));
    }

    // Note: We can't easily test the real MaxmindAsnLookup without a real database file
    // In a production environment, you would want to include a small test database
    // or mock the MaxMind reader for more comprehensive testing
}
