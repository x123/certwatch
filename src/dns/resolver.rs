use crate::{
    config::DnsConfig,
    core::{DnsInfo, DnsResolver},
    dns::DnsError,
    internal_metrics::Metrics,
};
use anyhow::Result;
use async_trait::async_trait;
use hickory_resolver::{
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    proto::xfer::Protocol,
    system_conf, TokioResolver,
};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{trace, warn};

/// DNS resolver implementation using trust-dns-resolver
pub struct HickoryDnsResolver {
    resolver: TokioResolver,
    _metrics: Arc<Metrics>,
}

impl HickoryDnsResolver {
    /// Creates a new DNS resolver from the application's DNS configuration.
    pub fn from_config(
        config: &DnsConfig,
        metrics: Arc<Metrics>,
    ) -> Result<(Self, Vec<SocketAddr>)> {
        let resolver_config = if let Some(resolver_addr_str) = &config.resolver {
            // If a specific resolver is provided, use it exclusively.
            let mut custom_config = ResolverConfig::new();
            let socket_addr: SocketAddr = resolver_addr_str.parse()?;
            custom_config.add_name_server(NameServerConfig::new(socket_addr, Protocol::Udp));
            custom_config
        } else {
            // Otherwise, load from system config
            let (system_config, _) = system_conf::read_system_conf()?;
            if system_config.name_servers().is_empty() {
                warn!("No system DNS servers found, falling back to Cloudflare DNS.");
                ResolverConfig::cloudflare()
            } else {
                system_config
            }
        };

        let mut resolver_config_with_no_search = ResolverConfig::new();
        for ns in resolver_config.name_servers() {
            resolver_config_with_no_search.add_name_server(ns.clone());
        }

        let mut nameservers: Vec<_> = resolver_config_with_no_search
            .name_servers()
            .iter()
            .map(|ns| ns.socket_addr)
            .collect();
        nameservers.sort();
        nameservers.dedup();

        let mut resolver_opts = ResolverOpts::default();
        // Set ndots to 1 to prevent the resolver from appending local search domains.
        // This ensures that we are always resolving the FQDN from the cert stream.
        resolver_opts.ndots = 1;

        // Override the timeout if specified in our application config.
        resolver_opts.timeout = Duration::from_millis(config.timeout_ms);

        // Enable caching if configured.
        if let Some(size) = config.cache_size {
            resolver_opts.cache_size = size;
        }

        let resolver = hickory_resolver::Resolver::builder_with_config(
            resolver_config_with_no_search,
            hickory_resolver::name_server::TokioConnectionProvider::default(),
        )
        .with_options(resolver_opts)
        .build();

        Ok((
            Self {
                resolver,
                _metrics: metrics,
            },
            nameservers,
        ))
    }
}

#[async_trait]
impl DnsResolver for HickoryDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        use hickory_resolver::proto::rr::RecordType;

        let start_time = Instant::now();
        let mut dns_info = DnsInfo::default();

        // Stage 1: Perform NS lookup to check for domain existence.
        match self.resolver.lookup(domain, RecordType::NS).await {
            Ok(lookup) => {
                dns_info.ns_records = lookup.into_iter().map(|r| r.to_string()).collect();
            }
            Err(e) => {
                let err_string = e.to_string();
                if is_nxdomain_error_str(&err_string) {
                    metrics::counter!("dns_queries_total", "status" => "nxdomain").increment(1);
                    // NXDOMAIN means we can stop here.
                    return Err(DnsError::Resolution(err_string));
                }
                // For other errors (like timeouts), we also fail fast.
                // The original error from hickory is sufficient.
                metrics::counter!("dns_queries_total", "status" => "failure").increment(1);
                return Err(DnsError::Resolution(err_string));
            }
        };

        // Stage 2: If NS lookup was successful, perform A and AAAA lookups.
        let (a_result, aaaa_result) = tokio::join!(
            self.resolver.lookup(domain, RecordType::A),
            self.resolver.lookup(domain, RecordType::AAAA),
        );

        let duration = start_time.elapsed();
        metrics::histogram!("dns_resolution_duration_seconds").record(duration.as_secs_f64());

        let mut primary_error = None;

        // Process A records
        match a_result {
            Ok(lookup) => {
                dns_info.a_records = lookup.into_iter().filter_map(|r| r.ip_addr()).collect();
            }
            Err(e) => {
                primary_error = Some(e);
            }
        }

        // Process AAAA records
        match aaaa_result {
            Ok(lookup) => {
                dns_info.aaaa_records = lookup.into_iter().filter_map(|r| r.ip_addr()).collect();
            }
            Err(e) => {
                if primary_error.is_none() {
                    primary_error = Some(e);
                } else {
                    trace!(domain, error = %e, "AAAA record lookup also failed");
                }
            }
        }

        // A resolution is successful if we get at least one A or AAAA record.
        if !dns_info.a_records.is_empty() || !dns_info.aaaa_records.is_empty() {
            if let Some(e) = primary_error {
                trace!(domain, error = %e, "A partial DNS failure occurred but was recovered");
            }
            metrics::counter!("dns_queries_total", "status" => "success").increment(1);
            return Ok(dns_info);
        }

        // If we are here, both A and AAAA lookups failed or returned empty.
        if let Some(err) = primary_error {
            let err_string = err.to_string();
            let status = if is_nxdomain_error_str(&err_string) {
                "nxdomain"
            } else if err_string.to_lowercase().contains("timeout") {
                "timeout"
            } else {
                "failure"
            };
            metrics::counter!("dns_queries_total", "status" => status).increment(1);
            return Err(DnsError::Resolution(err_string));
        }

        // This case means both lookups succeeded but returned no IP records.
        metrics::counter!("dns_queries_total", "status" => "nxdomain").increment(1);
        Err(DnsError::Resolution(format!(
            "No A or AAAA records found for {}",
            domain
        )))
    }
}

/// Checks if an `anyhow::Error` is an NXDOMAIN error.
fn is_nxdomain_error_str(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("nxdomain") || lower.contains("no records found")
}