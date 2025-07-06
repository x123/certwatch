use certwatch::enrichment::tsv_lookup::TsvAsnLookup;
use certwatch::core::EnrichmentProvider;
use std::net::IpAddr;

#[tokio::test]
async fn test_tsv_enrichment_provider() {
    let tsv_path = "tests/data/ip-to-asn-test.tsv";
    let enrichment_provider = TsvAsnLookup::new(tsv_path)
        .expect("Failed to create TSV enrichment provider");

    // Test Case 1: IPv4 address inside a range
    let ip1: IpAddr = "1.0.0.128".parse().unwrap();
    let result1 = enrichment_provider.enrich(ip1).await.unwrap();
    let data1 = result1.data.expect("Should find ASN for ip1");
    assert_eq!(data1.as_number, 13335);
    assert_eq!(data1.as_name, "CLOUDFLARENET");
    assert_eq!(data1.country_code.unwrap(), "US");

    // Test Case 2: IPv6 address
    let ip2: IpAddr = "2001:4860:4860::8888".parse().unwrap();
    let result2 = enrichment_provider.enrich(ip2).await.unwrap();
    let data2 = result2.data.expect("Should find ASN for ip2");
    assert_eq!(data2.as_number, 15169);
    assert_eq!(data2.as_name, "GOOGLE-IPV6");

    // Test Case 3: IP not in any range
    let ip3: IpAddr = "192.168.1.1".parse().unwrap();
    let result3 = enrichment_provider.enrich(ip3).await.unwrap();
    assert!(result3.data.is_none());

    // Test Case 4: "Not routed" entry should be skipped and not found
    let ip4: IpAddr = "0.0.0.0".parse().unwrap();
    let result4 = enrichment_provider.enrich(ip4).await.unwrap();
    assert!(result4.data.is_none());
}