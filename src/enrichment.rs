pub mod tsv_lookup;
// IP address enrichment services
//
// This module provides services for enriching IP addresses with ASN data.


#[cfg(test)]
mod tests {
    
    use crate::core::{AsnInfo, EnrichmentInfo, EnrichmentProvider};
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
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
            data: Some(AsnInfo {
                as_number: 15169,
                as_name: "Google LLC".to_string(),
                country_code: Some("US".to_string()),
            }),
        };

        provider.add_response(ip, expected_info.clone());

        let result = provider.enrich(ip).await.unwrap();
        assert_eq!(result.ip, expected_info.ip);
        let data = result.data.unwrap();
        assert_eq!(data.as_number, 15169);
        assert_eq!(data.country_code.unwrap(), "US");
    }

    #[tokio::test]
    async fn test_fake_enrichment_provider_partial_data() {
        let provider = FakeEnrichmentProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let expected_info = EnrichmentInfo {
            ip,
            data: Some(AsnInfo {
                as_number: 13335,
                as_name: "Cloudflare, Inc.".to_string(),
                country_code: None,
            }),
        };

        provider.add_response(ip, expected_info.clone());

        let result = provider.enrich(ip).await.unwrap();
        assert_eq!(result.ip, expected_info.ip);
        let data = result.data.unwrap();
        assert!(data.country_code.is_none());
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
        assert!(result.data.is_none());
    }
}
