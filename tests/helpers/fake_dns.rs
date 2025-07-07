#![allow(dead_code)]
//! A fake DNS resolver for testing purposes.

use async_trait::async_trait;
use certwatch::core::{DnsInfo, DnsResolver};
use certwatch::dns::DnsError;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, Default)]
pub struct FakeDnsResolver;

impl FakeDnsResolver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DnsResolver for FakeDnsResolver {
    async fn resolve(&self, _domain: &str) -> Result<DnsInfo, DnsError> {
        Ok(DnsInfo {
            a_records: vec![IpAddr::V4("127.0.0.1".parse::<Ipv4Addr>().unwrap())],
            aaaa_records: vec![],
            ns_records: vec![],
        })
    }
}