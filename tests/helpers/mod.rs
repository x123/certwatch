pub mod fs_watch;
pub mod mock_dns;
pub mod test_metrics;
pub mod fake_enrichment;
pub mod mock_ws;
pub mod mock_matcher;

use certwatch::core::{DnsResolver, EnrichmentProvider, PatternMatcher};
use fake_enrichment::FakeEnrichmentProvider;
use mock_dns::MockDnsResolver;
use mock_matcher::MockPatternMatcher;
use std::sync::Arc;

/// Creates a default set of mock dependencies for use in tests.
pub fn create_mock_dependencies() -> (
    Arc<dyn PatternMatcher + Send + Sync>,
    Arc<dyn DnsResolver + Send + Sync>,
    Arc<dyn EnrichmentProvider + Send + Sync>,
) {
    (
        Arc::new(MockPatternMatcher),
        Arc::new(MockDnsResolver::new()),
        Arc::new(FakeEnrichmentProvider::new()),
    )
}
pub mod mock_output;

pub mod app;
