#![allow(dead_code)]
//! A fake enrichment provider for testing purposes.

use anyhow::Result;
use async_trait::async_trait;
use certwatch::core::{EnrichmentInfo, EnrichmentProvider};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct FakeEnrichmentProvider {
    fail_on_enrich: Arc<AtomicBool>,
}

impl FakeEnrichmentProvider {
    pub fn new() -> Self {
        Self {
            fail_on_enrich: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_fail(&self, fail: bool) {
        self.fail_on_enrich.store(fail, Ordering::SeqCst);
    }
}

#[async_trait]
impl EnrichmentProvider for FakeEnrichmentProvider {
    async fn enrich(&self, ip: IpAddr) -> Result<EnrichmentInfo> {
        if self.fail_on_enrich.load(Ordering::SeqCst) {
            anyhow::bail!("Fake enrichment provider configured to fail");
        } else {
            Ok(EnrichmentInfo {
                ip,
                data: None,
            })
        }
    }
}