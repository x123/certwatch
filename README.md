# certwatch

`certwatch` is a high-performance Rust command-line tool that monitors Certificate Transparency logs in real-time. It connects to the certstream websocket, matches domains against thousands of regex patterns, enriches findings with DNS and ASN/GeoIP data, and sends alerts to configurable outputs. It is designed for security researchers to detect phishing, typosquatting, and other malicious domains as they are registered.

## Features

- **Real-time Monitoring:** Connects to the certstream network to see domains
from newly registered or renewed certificates.
- **High-Performance Pattern Matching:** Uses Rust's `regex::RegexSet` to match
against thousands of rules with very low latency.
- **Pattern Hot-Reloading:** Update regex pattern files on the fly without
restarting the service.
- **DNS & IP Enrichment:** Resolves domains (A, AAAA, NS records) and enriches
IPs with ASN and country data from a local TSV file.
- **Alert Deduplication:** Avoids alert fatigue by suppressing duplicate alerts
within a configurable time window.
- **Configurable Outputs:** Supports structured `JSON`, a compact `PlainText`
summary for `stdout`, and webhook notifications (for Slack notifications).
- **Logged Metrics:** Periodically log key operational metrics to the console for monitoring.

## Getting Started

### 1. Prerequisites

You need to have the Rust toolchain installed. You can find instructions at
[rust-lang.org](https://www.rust-lang.org/tools/install).

### 2. Build

Clone the repository and build the project in release mode:

```bash
git clone <repository_url>
cd certwatch
cargo build --release
```

The binary will be located at `./target/release/certwatch`.

### 3. Configuration

Before running, you need to set up your configuration file and data sources.

**a. Create `certwatch.toml`:**

Copy the example configuration file `certwatch.toml` to a new file (or use it
directly) and customize the settings.

**b. Create Pattern Files:**

In `certwatch.toml`, the `matching.pattern_files` key points to a list of text
files containing regex patterns. Each file should contain one regex pattern per
line. For example:

```text
# contents of phishing.txt
^.*(login|secure|account|webscr|signin).*paypal.*$
^.*(apple|icloud).*(login|support|verify).*$
```

**c. Create ASN Data File:**

The enrichment service requires a tab-separated value (TSV) file containing
IP-to-ASN mapping data. The path to this file is specified in `certwatch.toml`
under `enrichment.asn_tsv_path`.

The file **must** have the following five columns, separated by tabs:

```text
CIDR    AS_Number   AS_Name Country_Code    Description
```

A good starting point for this data is the combined IPv4 and IPv6 dataset
available from [https://iptoasn.com](https://iptoasn.com). Use the [Combined
IPv4+IPv6 to ASN map](https://iptoasn.com/data/ip2asn-combined.tsv.gz)

**Example `ip-to-asn.tsv`:**

```text
1.1.1.0/24	13335	CLOUDFLARENET	US	Cloudflare, Inc.
8.8.8.0/24	15169	GOOGLE	US	Google LLC
```

### 4. Run the Application

Once configured, you can run the application:

```bash
./target/release/certwatch
```

## Command-Line Arguments

You can override settings from `certwatch.toml` using command-line arguments.

| Flag | Description | Config Key |
| --- | --- | --- |
| `-c, --config <FILE>` | Path to a different TOML configuration file. | N/A |
| `--dns-resolver <IP>` | IP address of the DNS resolver to use (e.g., "8.8.8.8:53"). | `dns.resolver` |
| `--dns-timeout-ms <MS>` | Timeout for a single DNS query in milliseconds. | `dns.timeout_ms` |
| `--sample-rate <RATE>` | Sampling rate for the certstream (0.0 to 1.0). | `network.sample_rate` |
| `--log-metrics` | Periodically log key metrics to the console. | `log_metrics` |

**Example:**

```bash
./target/release/certwatch --dns-resolver 1.1.1.1:53 --log-metrics
```

## Further Reading

For a detailed description of the architecture, data structures, and advanced
features, please see the [**Final Product Requirements
Document**](docs/specs.md).
