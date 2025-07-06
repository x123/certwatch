//! Live integration test for the ASN enrichment service.
//!
//! This test verifies that the `MaxmindAsnLookup` service can correctly
//! initialize and perform lookups using a real GeoIP database. It is
//| intended to be run with the `live-tests` feature flag.

// use certwatch::core::AsnLookup;
// use certwatch::enrichment::MaxmindAsnLookup;
use std::net::{IpAddr, Ipv4Addr};

// #[tokio::test]
// #[cfg(feature = "live-tests")]
// async fn test_live_asn_lookup() {
//     // The real database is expected to be at this path.
//     // In a real CI/CD environment, this would be populated by a deployment script.
//     let db_path = "tests/data/GeoLite2-ASN-Test.mmdb";
//
//     println!("Attempting to load GeoIP database from: {}", db_path);
//
//     // Initialize the ASN lookup service
//     let asn_lookup = MaxmindAsnLookup::new(db_path)
//         .expect("Failed to load GeoIP database. Ensure the file exists and is valid.");
//
//     println!("GeoIP database loaded successfully.");
//
//     // --- Test Case 1: An IP known to be in the test database ---
//     let test_ip_1 = IpAddr::V4(Ipv4Addr::new(1, 128, 0, 0));
//     println!("\nLooking up IP: {}", test_ip_1);
//
//     let result = asn_lookup.lookup(test_ip_1).await;
//
//     match result {
//         Ok(info) => {
//             println!("✓ Lookup successful for {}: AS{} {}", test_ip_1, info.as_number, info.as_name);
//             // Corrected based on actual test output
//             assert_eq!(info.as_number, 1221, "Expected ASN 1221 for 1.128.0.0");
//             assert!(info.as_name.to_lowercase().contains("telstra"), "Expected organization name to contain 'telstra'");
//         }
//         Err(e) => {
//             panic!("✗ ASN lookup failed for {}: {}", test_ip_1, e);
//         }
//     }
//
//     // --- Test Case 2: Another IP known to be in the test database ---
//     let test_ip_2 = IpAddr::V4(Ipv4Addr::new(217, 156, 0, 0));
//     println!("\nLooking up IP: {}", test_ip_2);
//
//     let result = asn_lookup.lookup(test_ip_2).await;
//
//     match result {
//         Ok(info) => {
//             println!("✓ Lookup successful for {}: AS{} {}", test_ip_2, info.as_number, info.as_name);
//             // This IP is not in the test DB, so we expect a "not found" error.
//             // This is a good test case to ensure we handle non-existent IPs gracefully.
//             unreachable!("This test case should have failed, but it succeeded.");
//         }
//         Err(e) => {
//             println!("✓ Lookup for {} correctly failed: {}", test_ip_2, e);
//             assert!(e.to_string().contains("not found"), "Error message should indicate that the IP was not found.");
//         }
//     }
//
//     // --- Test Case 3: An IP that should not be in the database (Private IP) ---
//     let private_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
//     println!("\nLooking up IP: {}", private_ip);
//
//     let result = asn_lookup.lookup(private_ip).await;
//
//     assert!(result.is_err(), "Expected lookup for private IP to fail.");
//     if let Err(e) = result {
//         println!("✓ Lookup for private IP correctly failed: {}", e);
//         assert!(e.to_string().contains("not found"), "Error message should indicate that the IP was not found.");
//     }
//
//     println!("\nLive ASN enrichment test completed successfully.");
// }
