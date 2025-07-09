# certwatch

![certwatch logo](certwatch.png)

--- 
`certwatch` is a high-performance Rust command-line tool designed for real-time
monitoring of Certificate Transparency logs. It helps security researchers,
analysts, and developers detect and respond to potentially malicious domains
with newly registered or renewed certificates, such as those used in phishing
or typosquatting attacks. By connecting to the certstream websocket,
`certwatch` efficiently matches domains against thousands of user-defined regex
patterns, enriches findings with crucial DNS and ASN/GeoIP data, and dispatches
alerts to configurable outputs.

## Features

`certwatch` offers a robust set of features to provide comprehensive domain
monitoring:

*   **Real-time Monitoring:** Connects to a certstream websocket server to
process domains from newly registered or renewed certificates as they appear.
*   **High-Performance Pattern Matching:** Leverages Rust's `regex::RegexSet`
for extremely fast matching against thousands of rules with minimal latency.
*   **High-Performance Pre-emptive Filtering:** Uses a global `ignore` list,
powered by a `RegexSet`, to discard uninteresting domains from high-volume
sources (e.g., `*.google.com`) before any other processing occurs.
*   **DNS & IP Enrichment:** Resolves domains (A, AAAA, NS records) and
enriches associated IPs with ASN and country data using a local TSV file for
rapid lookups.
*   **Alert Deduplication:** Prevents alert fatigue by suppressing duplicate
alerts within a configurable time window.
*   **Configurable Outputs:** Supports various output formats including
structured `JSON`, a compact `PlainText` summary for `stdout`, and webhook
notifications (e.g., for Slack integration).
*   **Asynchronous & Non-Blocking Architecture:** Utilizes a fully
non-blocking, asynchronous design for efficient handling of all I/O-bound
operations, ensuring high performance under heavy load.
*   **Pattern Hot-Reloading:** Enables dynamic updates to regex pattern files
with a seamless switchover, allowing for continuous monitoring without
interrupting the data stream or requiring a service restart.
*   **Logged Metrics:** Periodically logs key operational metrics to the
console, providing insights into the application's performance and activity.

## Getting Started

To get `certwatch` up and running, follow these steps:

### 1. Prerequisites

You need to have the Rust toolchain installed. You can find detailed
installation instructions at
[rust-lang.org](https://www.rust-lang.org/tools/install).

### 2. Build

Clone the repository and build the project in release mode for optimal
performance:

```bash
git clone https://github.com/x123/certwatch.git
cd certwatch
cargo build --release
```

The compiled binary will be located at `./target/release/certwatch`.

### 3. Configuration

Before running `certwatch`, you need to set up your configuration file and data
sources.

**a. Create `certwatch.toml`:**

`certwatch` uses a `certwatch.toml` file for its configuration. You can copy
the example configuration file from [certwatch-example.toml](./certwatch-example.toml)

```toml
# example certwatch.toml configuration

[core]
# Valid levels: debug, info, warn, error
log_level = "info"

# The number of concurrent domain processing tasks.
# If commented out, defaults to (number of CPU cores).
# concurrency = 16

[metrics]
log_metrics = true
log_aggregation_seconds = 10

[performance]
queue_capacity = 100000

[network]
# REQUIRED: A running certstream-server certstream_url. Use
# https://github.com/CaliDog/certstream-server or similar.
certstream_url = "wss://127.0.0.1:8181/domains-only"
sample_rate = 1.0 # 1.0 = 100% sampling, 0.01 = 1% sampling
allow_invalid_certs = true

[dns]
# Optional: Specify a custom DNS resolver. If commented out, uses the system default.
# resolver = "192.168.1.1:53"

# Optional: Specify the timeout for a single DNS query in milliseconds.
# timeout_ms = 5000

# DNS retry and backoff settings.
standard_retries = 3
standard_initial_backoff_ms = 500
nxdomain_retries = 5
nxdomain_initial_backoff_ms = 10000

[dns.health]
# The failure rate threshold to trigger the unhealthy state (e.g., 0.95 for 95%).
failure_threshold = 0.95
# The time window in seconds to consider for the failure rate calculation.
window_seconds = 120
# A known-good domain to resolve to check for recovery.
recovery_check_domain = "google.com"

[enrichment]
# REQUIRED: Path to the TSV ASN database file.
# The file must be tab-separated with 5 columns:
# CIDR, AS_Number, AS_Name, Country_Code, Description
# The https://iptoasn.com/data/ip2asn-combined.tsv.gz from https://iptoasn.com is a
# compatible dataset for enrichment
asn_tsv_path = "enrichment/ip2asn-combined.tsv"

[output]
# The format to use for stdout output. Can be "Json" or "PlainText".
format = "PlainText"

# Optional: webhooks
# slack = { webhook_url = "https://hooks.slack.com/services/..." }

[deduplication]
cache_size = 100000
cache_ttl_seconds = 3600
```

**b. Create Rule Files:**

Detection logic is defined using advanced rule files. These YAML files are
**required** for `certwatch` to perform any matching. They support combining
conditions using `all` (AND), `any` (OR), and nested boolean logic, providing a
powerful way to define precise detection rules. They also support a global
`ignore` list for high-performance pre-filtering.

**Example `rules/advanced-logic.yml`:**

```yaml
# A list of regex patterns to ignore before any processing.
# This is highly efficient for filtering out high-volume, trusted domains.
ignore:
  - 'google\.com$'
  - 'facebook\.com$'
  - 'cloudfront\.net$'

# The list of detection rules.
rules:
  - name: "Generic Phishing Domain"
    # This rule triggers if a domain contains 'login' AND is NOT on a major cloud provider's network.
    all:
      - domain_regex: '(login|signin|account|secure)'
      - not_asns: [15169, 16509, 14618, 396982] # Google, Amazon, Microsoft, Oracle Cloud

  - name: "Suspicious TLD or High-Risk ASN"
    # This rule triggers if a domain uses a suspicious TLD OR originates from a specific ASN.
    any:
      - domain_regex: '\.(xyz|top|online|club)$'
      - asns: [20473] # AS20473 (CHOOPA) is often associated with bulletproof hosting.

  - name: "Complex Bank Phish"
    # This rule demonstrates nested logic.
    # It looks for domains that contain a bank name AND either are on a non-corporate
    # network OR use a suspicious TLD.
    all:
      - domain_regex: '(chase|wellsfargo|bankofamerica)'
      - any:
          - not_asns: [7843, 3589, 7132] # ASNs for Chase, Wells Fargo, Bank of America
          - domain_regex: '\.(biz|info)$'
```

#### Performance Benefits of Pre-emptive Filtering

The `ignore` list provides a significant performance advantage. It is compiled
into a single, highly-optimized `regex::RegexSet`. This allows `certwatch` to
check each incoming domain against a large set of "known good" patterns in a
single, very fast operation.

If a domain matches the ignore list, it is immediately discarded. This avoids
the more expensive downstream processing steps, such as DNS resolution, ASN
enrichment, and evaluation against more complex rules. For high-volume feeds,
this can reduce CPU and network load considerably.

**c. Supply ASN Data File:**

The enrichment service requires a tab-separated value (TSV) file containing
IP-to-ASN mapping data. The path to this file is specified in `certwatch.toml`
under `enrichment.asn_tsv_path`.

A compatible dataset is the [Combined IPv4+IPv6 to ASN map](https://iptoasn.com/data/ip2asn-combined.tsv.gz) dataset from
[ip2asn.com](https://ip2asn.com)

The file **must** have the following five columns, separated by tabs:

```text
start_ip    end_ip  asn country description
```

**Example `ip-to-asn.tsv`:**

```text
1.0.0.0	1.0.0.255	13335	US	CLOUDFLARENET
8.8.8.0	8.8.8.255	15169	US	GOOGLE
```

### 4. Run the Application

Once configured, you can run the application. All configuration is handled
via the `certwatch.toml` file.

```bash
./target/release/certwatch
```

By default, `certwatch` will look for a `certwatch.toml` file in the current
directory.

## Command-Line Arguments

The application's behavior is controlled almost exclusively by the `certwatch.toml`
file to ensure configuration is explicit and reproducible. The only command-line
arguments are:

| Flag | Description |
| --- | --- |
| `--config-file <PATH>` | Path to a specific TOML configuration file. Defaults to `./certwatch.toml`. |
| `--test-mode` | Runs the application in a test mode, which may alter certain behaviors (e.g., not connecting to live services). This is intended for internal testing. |

**Example:**

To run the application with a configuration file from a different location:
```bash
certwatch --config-file /etc/certwatch/production.toml
```

## Contributing

We welcome contributions to `certwatch`! If you'd like to contribute, please consider:

*   Reporting bugs or suggesting new features by opening an issue.
*   Submitting pull requests for bug fixes or new functionalities.

Please refer to `CONTRIBUTING.md` (coming soon!) for detailed guidelines on how to contribute.

## License

This project is licensed under the MIT License. See the `LICENSE` file for details.

## Further Reading

For a detailed description of the architecture, data structures, and advanced
features, please see the [**Final Product Requirements
Document**](docs/specs.md).

For the implementation plan, see the [**Epics**](docs/plan.md).
