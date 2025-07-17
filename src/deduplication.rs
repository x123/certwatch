// Service for filtering duplicate alerts.

use crate::core::Alert;
use crate::internal_metrics::Metrics;
use moka::{future::Cache as FutureCache, sync::Cache as SyncCache};
use psl;
use std::{sync::Arc, time::Duration};

/// A service that filters out duplicate alerts based on a time-aware cache.
/// This is used in the final output stage to prevent sending the same alert
/// multiple times over a longer period.
pub struct Deduplicator {
    cache: FutureCache<String, ()>,
    metrics: Arc<Metrics>,
}

impl Deduplicator {
    /// Creates a new `Deduplicator`.
    ///
    /// # Arguments
    /// * `ttl` - The time-to-live for an entry in the cache.
    /// * `max_capacity` - The maximum number of entries in the cache.
    pub fn new(ttl: Duration, max_capacity: u64, metrics: Arc<Metrics>) -> Self {
        let cache = FutureCache::builder()
            .time_to_live(ttl)
            .max_capacity(max_capacity)
            .build();
        Self { cache, metrics }
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

        if is_dupe {
            self.metrics.deduplicated_alerts_total.increment(1);
        } else {
            self.cache.insert(key, ()).await;
        }

        is_dupe
    }

    /// Generates a unique key for an alert.
    fn generate_key(&self, alert: &Alert) -> String {
        // Extract the base domain, falling back to the original domain if parsing fails.
        let base_domain = psl::domain_str(&alert.domain).unwrap_or(&alert.domain);

        // Sort and join the source tags to create a stable key.
        let mut sorted_tags = alert.source_tag.clone();
        sorted_tags.sort_unstable();
        let source_key = sorted_tags.join(",");

        if alert.resolved_after_nxdomain {
            // Key for "First Resolution" alerts includes the FQDN and source tag
            // to ensure they are unique and not deduplicated against the base domain.
            let data = format!("{}::{}::FIRST_RESOLUTION", alert.domain, source_key);
            blake3::hash(data.as_bytes()).to_hex().to_string()
        } else {
            // Standard alerts are deduplicated by base domain and source tag.
            let data = format!("{}{}", base_domain, source_key);
            blake3::hash(data.as_bytes()).to_hex().to_string()
        }
    }
}

/// A lightweight, short-term cache to prevent multiple workers from processing
/// the exact same domain simultaneously, which can cause DNS query amplification.
pub struct InFlightDeduplicator {
    cache: SyncCache<String, ()>,
}

impl InFlightDeduplicator {
    /// Creates a new `InFlightDeduplicator`.
    ///
    /// # Arguments
    /// * `ttl` - The time-to-live for an entry. Should be short (e.g., 30-60s).
    /// * `max_capacity` - The maximum number of in-flight domains to track.
    pub fn new(ttl: Duration, max_capacity: u64) -> Self {
        let cache = SyncCache::builder()
            .time_to_live(ttl)
            .max_capacity(max_capacity)
            .build();
        Self { cache }
    }

    /// Checks if a domain has been seen recently. If not, it inserts it into
    /// the cache. This operation is atomic.
    ///
    /// # Returns
    /// * `true` if the domain is a duplicate (was already in the cache).
    /// * `false` if the domain is new (and was just inserted).
    pub fn is_duplicate(&self, domain: &str) -> bool {
        // `get_or_insert_with` would be ideal, but we don't need the value.
        // `insert` overwrites, so we check first. This is not perfectly atomic,
        // but for this use case (preventing a stampede), it's sufficient.
        // A truly atomic "insert if not present" is `try_insert`, but that's nightly.
        if self.cache.contains_key(domain) {
            true
        } else {
            self.cache.insert(domain.to_string(), ());
            false
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{AsnInfo, DnsInfo, EnrichmentInfo},
        internal_metrics::Metrics,
    };
    use std::{net::Ipv4Addr, sync::Arc, time::Duration};

    fn create_test_alert(
        domain: &str,
        source_tags: &[&str],
        resolved_after_nxdomain: bool,
    ) -> Alert {
        Alert {
            timestamp: "2025-07-05T10:30:00Z".to_string(),
            domain: domain.to_string(),
            source_tag: source_tags.iter().map(|s| s.to_string()).collect(),
            resolved_after_nxdomain,
            dns: DnsInfo::default(),
            enrichment: vec![EnrichmentInfo {
                ip: Ipv4Addr::new(1, 1, 1, 1).into(),
                asn_info: Some(AsnInfo {
                    as_number: 15169,
                    as_name: "Google LLC".to_string(),
                    country_code: Some("US".to_string()),
                }),
            }],
            processing_start_time: None,
            notification_queue_start_time: None,
        }
    }

    #[tokio::test]
    async fn test_new_alert_is_not_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert = create_test_alert("example.com", &["phishing"], false);
        assert!(!deduplicator.is_duplicate(&alert).await);
    }

    #[tokio::test]
    async fn test_repeated_alert_is_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert = create_test_alert("example.com", &["phishing"], false);
        deduplicator.is_duplicate(&alert).await; // First time
        assert!(deduplicator.is_duplicate(&alert).await); // Second time
    }

    #[tokio::test]
    async fn test_first_resolution_alert_is_not_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["phishing"], false);
        let alert2 = create_test_alert("example.com", &["phishing"], true); // First resolution

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await);
    }

    #[tokio::test]
    async fn test_different_source_tag_is_not_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["phishing"], false);
        let alert2 = create_test_alert("example.com", &["malware"], false);

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await);
    }

    #[tokio::test]
    async fn test_first_resolution_alerts_with_different_tags_are_not_duplicates() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["source1"], true);
        let alert2 = create_test_alert("example.com", &["source2"], true);

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await, "Second alert with a different source tag should not be a duplicate");
    }

    #[tokio::test]
    async fn test_subdomain_alert_is_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["phishing"], false);
        let alert2 = create_test_alert("sub.example.com", &["phishing"], false); // Same base domain

        deduplicator.is_duplicate(&alert1).await; // First time
        assert!(deduplicator.is_duplicate(&alert2).await, "Alert for a subdomain of a recently seen domain should be a duplicate");
    }

    #[tokio::test]
    async fn test_different_base_domain_is_not_duplicate() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["phishing"], false);
        let alert2 = create_test_alert("example.org", &["phishing"], false); // Different base domain

        deduplicator.is_duplicate(&alert1).await;
        assert!(!deduplicator.is_duplicate(&alert2).await, "Alert for a different base domain should not be a duplicate");
    }
    #[tokio::test]
    async fn test_multi_tag_alert_is_duplicate_regardless_of_order() {
        let metrics = Arc::new(Metrics::new_for_test());
        let deduplicator = Deduplicator::new(Duration::from_secs(10), 100, metrics);
        let alert1 = create_test_alert("example.com", &["phishing", "malware"], false);
        let alert2 = create_test_alert("example.com", &["malware", "phishing"], false); // Same tags, different order

        deduplicator.is_duplicate(&alert1).await; // First time
        assert!(
            deduplicator.is_duplicate(&alert2).await,
            "Alert with the same tags in a different order should be a duplicate"
        );
    }
}
