//! A high-performance, in-memory ASN lookup service using an interval map
//! for IP range lookups from a TSV file.

use crate::core::{AsnInfo, EnrichmentInfo, EnrichmentProvider};
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use rangemap::RangeMap;
use serde::Deserialize;
use std::net::IpAddr;
use std::path::Path;

/// The main struct for the TSV-based ASN lookup service.
/// It holds an interval map (`RangeMap`) for fast IP range lookups.
#[derive(Debug, Clone)]
pub struct TsvAsnLookup {
    map: RangeMap<u128, AsnInfo>,
}

impl TsvAsnLookup {
    /// Creates a new `TsvAsnLookup` service by loading and parsing records
    /// from a TSV file at the given path.
    pub fn new_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Loading ASN data from TSV file: {:?}", path.as_ref());
        let file = std::fs::File::open(path)?;
        Self::new_from_reader(file)
    }

    /// Creates a new `TsvAsnLookup` service by parsing records from a
    /// reader (e.g., a file or an in-memory string).
    pub fn new_from_reader<R: std::io::Read>(reader: R) -> Result<Self> {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(false)
            .from_reader(reader);

        let mut map = RangeMap::new();
        for result in reader.deserialize() {
            let row: TsvRow = result?;

            // Skip "Not routed" entries
            if row.asn == 0 {
                continue;
            }

            let start_ip: IpAddr = row.start_ip.parse()?;
            let end_ip: IpAddr = row.end_ip.parse()?;

            let start = ip_to_u128(start_ip);
            let end = ip_to_u128(end_ip);

            let data = AsnInfo {
                as_number: row.asn,
                as_name: row.description,
                country_code: Some(row.country),
            };

            // Insert the range and its corresponding data into the map.
            // The range is exclusive on the end, so we add 1.
            map.insert(start..end + 1, data);
        }

        debug!(
            "Loaded and built interval map with {} ASN records.",
            map.len()
        );

        Ok(Self { map })
    }

    /// Finds the ASN data for a given IP address using the interval map.
    ///
    /// # Arguments
    /// * `ip` - The IP address to look up.
    fn find(&self, ip: IpAddr) -> Option<AsnInfo> {
        let ip_num = ip_to_u128(ip);
        // `get` returns the value associated with the range containing the key.
        self.map.get(&ip_num).cloned()
    }
}

#[async_trait]
impl EnrichmentProvider for TsvAsnLookup {
    async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo> {
        Ok(EnrichmentInfo {
            ip,
            data: self.find(ip),
        })
    }
}

/// A helper struct for deserializing a row from the TSV file using the `csv` crate.
#[derive(Debug, Deserialize)]
struct TsvRow {
    start_ip: String,
    end_ip: String,
    asn: u32,
    country: String,
    description: String,
}

/// Converts an IpAddr to its u128 representation.
fn ip_to_u128(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(ipv4) => u32::from(ipv4).into(),
        IpAddr::V6(ipv6) => u128::from(ipv6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn test_data_path() -> String {
        "tests/data/ip-to-asn-test.tsv".to_string()
    }

    #[test]
    fn test_load_and_parse_tsv() {
        let lookup = TsvAsnLookup::new_from_path(test_data_path()).expect("Failed to load test data");
        // The new test file has 6 lines, one of which is "Not routed" and should be skipped.
        assert_eq!(lookup.map.len(), 5);
    }

    #[test]
    fn test_find_ip() {
        let lookup = TsvAsnLookup::new_from_path(test_data_path()).expect("Failed to load test data");

        // Test Case 1: IPv4 address inside a range
        let ip1: IpAddr = "1.0.0.128".parse().unwrap();
        let result1 = lookup.find(ip1).expect("Should find ASN for ip1");
        assert_eq!(result1.as_number, 13335);
        assert_eq!(result1.as_name, "CLOUDFLARENET");
        assert_eq!(result1.country_code.unwrap(), "US");

        // Test Case 2: IPv6 address
        let ip2: IpAddr = "2001:4860:4860::8888".parse().unwrap();
        let result2 = lookup.find(ip2).expect("Should find ASN for ip2");
        assert_eq!(result2.as_number, 15169);
        assert_eq!(result2.as_name, "GOOGLE-IPV6");

        // Test Case 3: IP not in any range
        let ip3: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(lookup.find(ip3).is_none());

        // Test Case 4: "Not routed" entry should be skipped and not found
        let ip4: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(lookup.find(ip4).is_none());
    }
}
    #[test]
    fn test_lookup_from_in_memory_tsv() {
        let tsv_data = "8.8.8.0\t8.8.8.255\t15169\tUS\tGOOGLE";
        let lookup = TsvAsnLookup::new_from_reader(tsv_data.as_bytes())
            .expect("Failed to load in-memory data");

        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        let result = lookup.find(ip).expect("Should find ASN for ip");
        assert_eq!(result.as_number, 15169);
        assert_eq!(result.as_name, "GOOGLE");
    }
    #[test]
    fn test_malformed_tsv_jagged_rows() {
        let tsv_data = "8.8.8.0\t8.8.8.255\t15169\tUS"; // Missing the description field
        let result = TsvAsnLookup::new_from_reader(tsv_data.as_bytes());
        assert!(result.is_err());
    }
    #[test]
    fn test_malformed_tsv_invalid_ip() {
        let tsv_data = "not-an-ip\t8.8.8.255\t15169\tUS\tGOOGLE";
        let result = TsvAsnLookup::new_from_reader(tsv_data.as_bytes());
        assert!(result.is_err());
    }