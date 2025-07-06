use metrics;
// Service for filtering duplicate alerts.

use crate::core::Alert;
use moka::future::Cache;
use std::time::Duration;

/// A service that filters out duplicate alerts based on a time-aware cache.
pub struct Deduplicator {
    cache: Cache<String, ()>,
}

impl Deduplicator {
    /// Creates a new `Deduplicator`.
    ///
    /// # Arguments
    /// * `ttl` - The time-to-live for an entry in the cache.
    /// * `max_capacity` - The maximum number of entries in the cache.
    pub fn new(ttl: Duration, max_capacity: u64) -> Self {
        let cache = Cache::builder()
            .time_to_live(ttl)
            .max_capacity(max_capacity)
            .build();
        Self { cache }
    }

    /// Checks if an alert is a duplicate.
    ///
    /// An alert is considered a duplicate if an alert with the same key has been
    /// seen within the `ttl` period.
    ///
    /// # Returns
    /// * `true` if the alert is a duplicate.
    /// * `false` if the alert is not a duplicate.
    pub async fn is_duplicate(&self, alert: &Alert) -> bool {
        let key = self.generate_key(alert);
        let is_dupe = self.cache.contains_key(&key);
        
        if !is_dupe {
            self.cache.insert(key, ()).await;
        }
        
        metrics::gauge!("deduplication_cache_entries").set(self.cache.entry_count() as f64);
        
        is_dupe
    }

    /// Generates a unique key for an alert.
    fn generate_key(&self, alert: &Alert) -> String {
        if alert.resolved_after_nxdomain {
            // Key for "First Resolution" alerts includes the source tag to ensure
            // they are unique per source.
            let data = format!(
                "{}::{}::FIRST_RESOLUTION",
                alert.domain, alert.source_tag
            );
            blake3::hash(data.as_bytes()).to_hex().to_string()
        } else {
            let data = format!("{}{}", alert.domain, alert.source_tag);
            blake3::hash(data.as_bytes()).to_hex().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AsnInfo, DnsInfo, EnrichmentInfo};
    use std::net::Ipv4Addr;
    use std::time::Duration;

    fn create_test_alert(domain: &str, source_tag: &str, resolved_after_nxdomain: bool) -> Alert {
        Alert {
            timestamp: "2025-07-05T10:30:00Z".to_string(),
            domain: domain.to_string(),
            source_tag: source_tag.to_string(),
            resolved_after_nxdomain,
            dns: DnsInfo::default(),
            enrichment: vec![EnrichmentInfo {
                ip: Ipv4Addr::new(1, 1, 1, 1).into(),
                data: Some(AsnInfo {
                    as_number: 15169,
                    as_name: "Google LLC".to_string(),
                    country_code: Some("US".to_string()),
                }),
            }],
        }
    }

    #[tokio::test]
    async fn test_new_alert_is_not_duplicate() {
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100);
        let alert = create_test_alert("example.com", "phishing", false);
        assert!(!deduplicator.is_duplicate(&alert).await);
    }

    #[tokio::test]
    async fn test_repeated_alert_is_duplicate() {
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100);
        let alert = create_test_alert("example.com", "phishing", false);
        deduplicator.is_duplicate(&alert).await; // First time
        assert!(deduplicator.is_duplicate(&alert).await); // Second time
    }

    #[tokio::test]
    async fn test_first_resolution_alert_is_not_duplicate() {
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100);
        let alert1 = create_test_alert("example.com", "phishing", false);
        let alert2 = create_test_alert("example.com", "phishing", true); // First resolution

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await);
    }

    #[tokio::test]
    async fn test_different_source_tag_is_not_duplicate() {
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100);
        let alert1 = create_test_alert("example.com", "phishing", false);
        let alert2 = create_test_alert("example.com", "malware", false);

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await);
    }

    #[tokio::test]
    async fn test_first_resolution_alerts_with_different_tags_are_not_duplicates() {
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100);
        let alert1 = create_test_alert("example.com", "source1", true);
        let alert2 = create_test_alert("example.com", "source2", true);

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await, "Second alert with a different source tag should not be a duplicate");
    }
}
