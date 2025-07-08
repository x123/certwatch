//! Pre-flight health check for enrichment data sources.

use crate::config::Config;
use crate::core::EnrichmentProvider;
use crate::enrichment::{NoOpEnrichmentProvider, TsvAsnLookup};
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

/// Performs a pre-flight check on the enrichment data source.
///
/// # Arguments
/// * `config` - The application configuration.
///
/// # Returns
/// * A `Result` containing a configured `EnrichmentProvider` on success,
///   or an error if the check fails.
pub async fn startup_check(config: &Config) -> Result<Arc<dyn EnrichmentProvider>> {
    if let Some(tsv_path) = &config.enrichment.asn_tsv_path {
        info!("ASN TSV Path specified: {}", tsv_path.display());
        if !tsv_path.exists() {
            anyhow::bail!("TSV database not found at {:?}", tsv_path);
        }
        info!("Loading ASN database from {:?}...", tsv_path);
        let provider = Arc::new(TsvAsnLookup::new_from_path(tsv_path)?);
        info!("ASN database loaded successfully.");
        Ok(provider)
    } else {
        info!("No ASN database configured, proceeding without enrichment.");
        Ok(Arc::new(NoOpEnrichmentProvider))
    }
}