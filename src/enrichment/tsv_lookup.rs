//! A high-performance, in-memory ASN lookup service using a sorted list of IP ranges from a TSV file.

use crate::core::{AsnData, EnrichmentProvider};
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use serde::Deserialize;
use std::net::IpAddr;
use std::path::Path;

/// Represents a single record in the IP-to-ASN database.
/// The start and end IPs are stored as u128 for unified, efficient comparison.
#[derive(Debug, Clone)]
struct AsnRecord {
    start: u128,
    end: u128,
    data: AsnData,
}

/// The main struct for the TSV-based ASN lookup service.
/// It holds a sorted vector of `AsnRecord` for fast binary searching.
#[derive(Debug, Clone)]
pub struct TsvAsnLookup {
    records: Vec<AsnRecord>,
}

impl TsvAsnLookup {
    /// Creates a new `TsvAsnLookup` service by loading and parsing records from a TSV file.
    ///
    /// # Arguments
    /// * `path` - The path to the TSV file containing the IP range data.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Loading ASN data from TSV file: {:?}", path.as_ref());
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(false)
            .from_path(path)?;

        let mut records = Vec::new();
        for result in reader.deserialize() {
            let row: TsvRow = result?;

            // Skip "Not routed" entries
            if row.asn == 0 {
                continue;
            }

            let start_ip: IpAddr = row.start_ip.parse()?;
            let end_ip: IpAddr = row.end_ip.parse()?;

            records.push(AsnRecord {
                start: ip_to_u128(start_ip),
                end: ip_to_u128(end_ip),
                data: AsnData {
                    as_number: row.asn,
                    as_name: row.description,
                },
            });
        }

        // Sort records by the starting IP address for binary search
        records.sort_by_key(|r| r.start);

        debug!("Loaded and sorted {} ASN records.", records.len());

        Ok(Self { records })
    }

    /// Finds the ASN data for a given IP address using a binary search.
    ///
    /// # Arguments
    /// * `ip` - The IP address to look up.
    fn find(&self, ip: IpAddr) -> Option<AsnData> {
        let ip_num = ip_to_u128(ip);

        // `binary_search_by_key` finds a record where `ip_num` is within the range.
        // Since the ranges are sorted by `start`, we can find the insertion point.
        let search_result = self.records.binary_search_by_key(&ip_num, |record| record.start);

        // The index we get from the search will either be a direct hit or the
        // index where the element could be inserted to maintain order.
        // If it's an `Err(idx)`, the correct range (if one exists) must be the
        // one at `idx - 1`.
        let potential_match_idx = match search_result {
            Ok(idx) => idx, // Direct hit on a range start
            Err(idx) => {
                if idx == 0 {
                    return None; // IP is smaller than any range start
                }
                idx - 1
            }
        };

        if let Some(record) = self.records.get(potential_match_idx) {
            // Check if the IP is within the found record's range.
            if ip_num >= record.start && ip_num <= record.end {
                return Some(record.data.clone());
            }
        }

        None
    }
}

#[async_trait]
impl EnrichmentProvider for TsvAsnLookup {
    async fn enrich(&self, ip: IpAddr) -> Result<crate::core::EnrichmentInfo> {
        // Placeholder: This will call the `find` method and format the result.
        Ok(crate::core::EnrichmentInfo {
            ip,
            asn: self.find(ip),
            geoip: None, // This provider only handles ASN data.
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
        "tests/data/ip2asn-combined-test.tsv".to_string()
    }

    #[test]
    fn test_load_and_parse_tsv() {
        let lookup = TsvAsnLookup::new(test_data_path()).expect("Failed to load test data");
        // The test file has 200 lines, but many are "Not routed" (ASN 0) and should be skipped.
        // The test file has 200 lines, but some are "Not routed" (ASN 0) and should be skipped.
        assert_eq!(lookup.records.len(), 163);

        // Verify the first record is sorted correctly
        let first_record = lookup.records.first().unwrap();
        assert_eq!(first_record.data.as_number, 9318); // 1.224.20.43
    }

    #[test]
    fn test_find_ip() {
        let lookup = TsvAsnLookup::new(test_data_path()).expect("Failed to load test data");

        // Test Case 1: IPv4 address inside a range
        let ip1: IpAddr = "64.165.249.100".parse().unwrap();
        let result1 = lookup.find(ip1).expect("Should find ASN for ip1");
        assert_eq!(result1.as_number, 19248);
        assert_eq!(result1.as_name, "ACSC1000");

        // Test Case 2: IPv6 address inside a range
        let ip2: IpAddr = "2800:860:7161::1".parse().unwrap();
        let result2 = lookup.find(ip2).expect("Should find ASN for ip2");
        assert_eq!(result2.as_number, 262197);
        assert_eq!(result2.as_name, "MILLICOM CABLE COSTA RICA S.A.");

        // Test Case 3: IP at the start of a range
        let ip3: IpAddr = "93.125.32.0".parse().unwrap();
        let result3 = lookup.find(ip3).expect("Should find ASN for ip3");
        assert_eq!(result3.as_number, 42772);

        // Test Case 4: IP at the end of a range
        let ip4: IpAddr = "93.125.32.255".parse().unwrap();
        let result4 = lookup.find(ip4).expect("Should find ASN for ip4");
        assert_eq!(result4.as_number, 42772);

        // Test Case 5: IP not in any range
        let ip5: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(lookup.find(ip5).is_none());

        // Test Case 6: Another IPv6 test
        let ip6: IpAddr = "2a02:26f7:f6d6::beef".parse().unwrap();
        let result6 = lookup.find(ip6).expect("Should find ASN for ip6");
        assert_eq!(result6.as_number, 20940);
        assert_eq!(result6.as_name, "AKAMAI-ASN1");
    }
}