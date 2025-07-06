//! Integration tests for the enrichment service using real MaxMind test data
//!
//! This test uses the official MaxMind test database to verify that our
//! MaxmindEnrichmentProvider implementation correctly parses and extracts ASN data.
//! It validates against the source JSON data to ensure accuracy.

use certwatch::enrichment::MaxmindEnrichmentProvider;
use certwatch::core::EnrichmentProvider;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use ipnetwork::IpNetwork;

/// Represents an ASN entry from the MaxMind test data JSON
#[derive(Debug, Deserialize)]
struct AsnTestEntry {
    autonomous_system_number: u32,
    autonomous_system_organization: Option<String>,
}

/// Test case extracted from the JSON source data
#[derive(Debug)]
struct TestCase {
    network: IpNetwork,
    expected_asn: u32,
    expected_org: Option<String>,
}

/// Load and parse the MaxMind test data JSON file, separated by IP version
fn load_test_data() -> Result<(Vec<TestCase>, Vec<TestCase>), Box<dyn std::error::Error>> {
    let json_content = std::fs::read_to_string("tests/data/GeoLite2-ASN-Test.json")?;
    let raw_data: Vec<HashMap<String, AsnTestEntry>> = serde_json::from_str(&json_content)?;
    
    let mut ipv4_test_cases = Vec::new();
    let mut ipv6_test_cases = Vec::new();
    
    for entry_map in raw_data {
        for (network_str, asn_entry) in entry_map {
            let network: IpNetwork = network_str.parse()?;
            let test_case = TestCase {
                network,
                expected_asn: asn_entry.autonomous_system_number,
                expected_org: asn_entry.autonomous_system_organization,
            };
            
            match network {
                IpNetwork::V4(_) => ipv4_test_cases.push(test_case),
                IpNetwork::V6(_) => ipv6_test_cases.push(test_case),
            }
        }
    }
    
    Ok((ipv4_test_cases, ipv6_test_cases))
}

#[tokio::test]
async fn test_maxmind_enrichment_with_real_db() {
    // Path to the test database
    let db_path = "tests/data/GeoLite2-ASN-Test.mmdb";
    
    // Create the enrichment service with the real test database
    // We pass the same path for the GeoIP db as a placeholder.
    let provider = MaxmindEnrichmentProvider::new(db_path, db_path)
        .expect("Failed to load MaxMind test database");

    // Load the ground-truth test data from JSON
    let (ipv4_test_cases, ipv6_test_cases) = load_test_data()
        .expect("Failed to load test data from JSON file");
    
    println!("Loaded {} IPv4 and {} IPv6 test cases from JSON source data", 
             ipv4_test_cases.len(), ipv6_test_cases.len());
    
    let mut successful_tests = 0;
    
    // Test a sample of IPv4 cases
    println!("\n=== Testing IPv4 addresses ===");
    for test_case in ipv4_test_cases.iter().take(5) {
        let test_ip = match test_case.network {
            IpNetwork::V4(net) => IpAddr::V4(net.network()),
            IpNetwork::V6(_) => unreachable!(),
        };
        
        let result = provider.enrich(test_ip).await.unwrap();
        
        assert_eq!(result.ip, test_ip);
        
        if let Some(data) = result.data {
            assert_eq!(data.as_number, test_case.expected_asn);
            if let Some(ref expected_org) = test_case.expected_org {
                assert_eq!(&data.as_name, expected_org);
            }
            successful_tests += 1;
            println!("✓ IPv4 test case passed for IP {}: AS{} {}", test_ip, data.as_number, data.as_name);
        } else {
            println!("- IPv4 test case skipped for IP {}: Not found in DB", test_ip);
        }
    }
    
    // Test a sample of IPv6 cases
    println!("\n=== Testing IPv6 addresses ===");
    for test_case in ipv6_test_cases.iter().take(5) {
        let test_ip = match test_case.network {
            IpNetwork::V4(_) => unreachable!(),
            IpNetwork::V6(net) => IpAddr::V6(net.network()),
        };
        
        let result = provider.enrich(test_ip).await.unwrap();
        
        assert_eq!(result.ip, test_ip);

        if let Some(data) = result.data {
            assert_eq!(data.as_number, test_case.expected_asn);
            if let Some(ref expected_org) = test_case.expected_org {
                assert_eq!(&data.as_name, expected_org);
            }
            successful_tests += 1;
            println!("✓ IPv6 test case passed for IP {}: AS{} {}", test_ip, data.as_number, data.as_name);
        } else {
            println!("- IPv6 test case skipped for IP {}: Not found in DB", test_ip);
        }
    }
    
    println!("\nTest summary: {} successful lookups", successful_tests);
    assert!(successful_tests >= 2, "Expected at least 2 successful lookups");
    
    // Test an IP that should NOT be in the database (private IP)
    let private_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let private_result = provider.enrich(private_ip).await.unwrap();
    
    assert!(private_result.data.is_none(), "Private IP should not have enrichment data");
    println!("✓ Private IP correctly not found in database");
}

#[tokio::test]
async fn test_maxmind_enrichment_invalid_database_path() {
    // Test that we get a proper error when the database file doesn't exist
    let result = MaxmindEnrichmentProvider::new("nonexistent/path.mmdb", "nonexistent/path.mmdb");
    
    assert!(result.is_err(), "Should fail when database file doesn't exist");
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Failed to open MaxMind ASN database"), 
           "Error should mention database opening failure: {}", error_msg);
    println!("✓ Invalid database path correctly handled");
}
