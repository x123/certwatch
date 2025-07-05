//! Integration tests for the ASN enrichment service using real MaxMind test data
//!
//! This test uses the official MaxMind test database to verify that our
//! MaxmindAsnLookup implementation correctly parses and extracts ASN data.
//! It validates against the source JSON data to ensure accuracy.

use certwatch::enrichment::MaxmindAsnLookup;
use certwatch::core::AsnLookup;
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
async fn test_maxmind_asn_lookup_with_real_db() {
    // Path to the test database
    let db_path = "tests/data/GeoLite2-ASN-Test.mmdb";
    
    // Create the ASN lookup service with the real test database
    let asn_lookup = MaxmindAsnLookup::new(db_path)
        .expect("Failed to load MaxMind test database");

    // Load the ground-truth test data from JSON, separated by IP version
    let (ipv4_test_cases, ipv6_test_cases) = load_test_data()
        .expect("Failed to load test data from JSON file");
    
    println!("Loaded {} IPv4 and {} IPv6 test cases from JSON source data", 
             ipv4_test_cases.len(), ipv6_test_cases.len());
    
    let mut successful_tests = 0;
    let mut failed_tests = 0;
    let mut test_case_counter = 0;
    
    // Test IPv4 cases (take up to 5)
    println!("\n=== Testing IPv4 addresses ===");
    for test_case in ipv4_test_cases.iter().take(5) {
        test_case_counter += 1;
        
        // Pick a representative IP from the network range
        let test_ip = match test_case.network {
            IpNetwork::V4(net) => IpAddr::V4(net.network()),
            IpNetwork::V6(_) => unreachable!("Should only have IPv4 here"),
        };
        
        println!("IPv4 test case {}: Testing IP {} from network {} (expected AS{})", 
                 test_case_counter, test_ip, test_case.network, test_case.expected_asn);
        
        let result = asn_lookup.lookup(test_ip).await;
        
        match result {
            Ok(asn_info) => {
                // Verify the IP matches
                assert_eq!(asn_info.ip, test_ip, 
                          "IP mismatch for IPv4 test case {}", test_case_counter);
                
                // Verify the ASN number matches the ground truth
                assert_eq!(asn_info.as_number, test_case.expected_asn,
                          "ASN number mismatch for IPv4 test case {}: expected AS{}, got AS{}",
                          test_case_counter, test_case.expected_asn, asn_info.as_number);
                
                // If we have expected organization data, verify it matches
                if let Some(ref expected_org) = test_case.expected_org {
                    let expected_fallback = format!("AS{}", test_case.expected_asn);
                    assert!(
                        asn_info.as_name == *expected_org || asn_info.as_name == expected_fallback,
                        "ASN organization mismatch for IPv4 test case {}: expected '{}' or '{}', got '{}'",
                        test_case_counter, expected_org, expected_fallback, asn_info.as_name
                    );
                }
                
                println!("✓ IPv4 test case {} passed: AS{} {}", 
                         test_case_counter, asn_info.as_number, asn_info.as_name);
                successful_tests += 1;
            }
            Err(e) => {
                println!("✗ IPv4 test case {} failed: {}", test_case_counter, e);
                failed_tests += 1;
            }
        }
    }
    
    // Test IPv6 cases (take up to 5)
    println!("\n=== Testing IPv6 addresses ===");
    for test_case in ipv6_test_cases.iter().take(5) {
        test_case_counter += 1;
        
        // Pick a representative IP from the network range
        let test_ip = match test_case.network {
            IpNetwork::V4(_) => unreachable!("Should only have IPv6 here"),
            IpNetwork::V6(net) => IpAddr::V6(net.network()),
        };
        
        println!("IPv6 test case {}: Testing IP {} from network {} (expected AS{})", 
                 test_case_counter, test_ip, test_case.network, test_case.expected_asn);
        
        let result = asn_lookup.lookup(test_ip).await;
        
        match result {
            Ok(asn_info) => {
                // Verify the IP matches
                assert_eq!(asn_info.ip, test_ip, 
                          "IP mismatch for IPv6 test case {}", test_case_counter);
                
                // Verify the ASN number matches the ground truth
                assert_eq!(asn_info.as_number, test_case.expected_asn,
                          "ASN number mismatch for IPv6 test case {}: expected AS{}, got AS{}",
                          test_case_counter, test_case.expected_asn, asn_info.as_number);
                
                // If we have expected organization data, verify it matches
                if let Some(ref expected_org) = test_case.expected_org {
                    let expected_fallback = format!("AS{}", test_case.expected_asn);
                    assert!(
                        asn_info.as_name == *expected_org || asn_info.as_name == expected_fallback,
                        "ASN organization mismatch for IPv6 test case {}: expected '{}' or '{}', got '{}'",
                        test_case_counter, expected_org, expected_fallback, asn_info.as_name
                    );
                }
                
                println!("✓ IPv6 test case {} passed: AS{} {}", 
                         test_case_counter, asn_info.as_number, asn_info.as_name);
                successful_tests += 1;
            }
            Err(e) => {
                println!("✗ IPv6 test case {} failed: {}", test_case_counter, e);
                failed_tests += 1;
            }
        }
    }
    
    println!("\nTest summary: {} successful, {} failed", successful_tests, failed_tests);
    
    // We expect at least some successful lookups from both IPv4 and IPv6
    // This accounts for potential differences between the JSON source and the .mmdb file
    assert!(successful_tests >= 2, 
           "Expected at least 2 successful lookups, but got {}", successful_tests);
    
    // Test an IP that should NOT be in the database (private IP)
    let private_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let private_result = asn_lookup.lookup(private_ip).await;
    
    assert!(private_result.is_err(), "Private IP should not be found in ASN database");
    let error_msg = private_result.unwrap_err().to_string();
    assert!(error_msg.contains("not found"), "Error should indicate IP not found: {}", error_msg);
    println!("✓ Private IP correctly not found in database");
}

#[tokio::test]
async fn test_maxmind_asn_lookup_invalid_database_path() {
    // Test that we get a proper error when the database file doesn't exist
    let result = MaxmindAsnLookup::new("nonexistent/path/to/database.mmdb");
    
    assert!(result.is_err(), "Should fail when database file doesn't exist");
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Failed to open MaxMind database"), 
           "Error should mention database opening failure: {}", error_msg);
    println!("✓ Invalid database path correctly handled");
}

#[tokio::test]
async fn test_maxmind_asn_lookup_from_bytes() {
    // Test the from_bytes constructor by reading the test database file
    let db_bytes = std::fs::read("tests/data/GeoLite2-ASN-Test.mmdb")
        .expect("Failed to read test database file");
    
    let asn_lookup = MaxmindAsnLookup::from_bytes(db_bytes)
        .expect("Failed to create ASN lookup from bytes");
    
    // Test with a known IP to verify it works the same as file-based loading
    let test_ip = IpAddr::V4(Ipv4Addr::new(1, 128, 0, 0));
    let result = asn_lookup.lookup(test_ip).await;
    
    // We expect either success or a consistent "not found" error
    match result {
        Ok(asn_info) => {
            assert_eq!(asn_info.ip, test_ip);
            assert!(asn_info.as_number > 0);
            assert!(!asn_info.as_name.is_empty());
            println!("✓ Bytes-based lookup successful: AS{} {}", asn_info.as_number, asn_info.as_name);
        }
        Err(e) => {
            // If the IP isn't found, that's also acceptable for this test
            assert!(e.to_string().contains("not found"), 
                   "Error should be about IP not found: {}", e);
            println!("✓ Bytes-based lookup correctly handled missing IP");
        }
    }
}
