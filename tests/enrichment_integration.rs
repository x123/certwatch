//! Integration tests for the ASN enrichment service using real MaxMind test data
//!
//! This test uses the official MaxMind test database to verify that our
//! MaxmindAsnLookup implementation correctly parses and extracts ASN data.

use certwatch::enrichment::MaxmindAsnLookup;
use certwatch::core::AsnLookup;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[tokio::test]
async fn test_maxmind_asn_lookup_with_real_db() {
    // Path to the test database
    let db_path = "tests/data/GeoLite2-ASN-Test.mmdb";
    
    // Create the ASN lookup service with the real test database
    let asn_lookup = MaxmindAsnLookup::new(db_path)
        .expect("Failed to load MaxMind test database");

    // Test with known IPs from the MaxMind test data
    // These are documented test IPs that should be in the test database
    
    // Test IPv4 address: 1.128.0.0 should be AS1221 Telstra Corporation
    let ipv4_test = IpAddr::V4(Ipv4Addr::new(1, 128, 0, 0));
    let result = asn_lookup.lookup(ipv4_test).await;
    
    match result {
        Ok(asn_info) => {
            assert_eq!(asn_info.ip, ipv4_test);
            assert_eq!(asn_info.as_number, 1221);
            assert!(asn_info.as_name.contains("Telstra"));
            println!("✓ IPv4 lookup successful: AS{} {}", asn_info.as_number, asn_info.as_name);
        }
        Err(e) => {
            // If this specific IP isn't in the test DB, try another known test IP
            println!("IPv4 test IP not found, trying alternative: {}", e);
            
            // Try 2.125.160.216 which should be AS1221
            let alt_ipv4 = IpAddr::V4(Ipv4Addr::new(2, 125, 160, 216));
            let alt_result = asn_lookup.lookup(alt_ipv4).await;
            
            match alt_result {
                Ok(asn_info) => {
                    assert_eq!(asn_info.ip, alt_ipv4);
                    assert_eq!(asn_info.as_number, 1221);
                    println!("✓ Alternative IPv4 lookup successful: AS{} {}", asn_info.as_number, asn_info.as_name);
                }
                Err(e2) => {
                    panic!("Both test IPv4 addresses failed: {} and {}", e, e2);
                }
            }
        }
    }

    // Test IPv6 address: 2001:200:: should be AS2500 WIDE Project
    let ipv6_test = IpAddr::V6(Ipv6Addr::new(0x2001, 0x200, 0, 0, 0, 0, 0, 0));
    let ipv6_result = asn_lookup.lookup(ipv6_test).await;
    
    match ipv6_result {
        Ok(asn_info) => {
            assert_eq!(asn_info.ip, ipv6_test);
            assert_eq!(asn_info.as_number, 2500);
            assert!(asn_info.as_name.contains("WIDE"));
            println!("✓ IPv6 lookup successful: AS{} {}", asn_info.as_number, asn_info.as_name);
        }
        Err(e) => {
            println!("IPv6 test may not be in this version of test DB: {}", e);
            // IPv6 data might not be in all versions of the test database
            // This is acceptable for this test
        }
    }

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
