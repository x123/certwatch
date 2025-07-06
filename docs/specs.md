# **Final Product Requirements Document: `certwatch`**

## 1. Overview

`certwatch` is a high-performance Rust command-line tool that monitors Certificate Transparency logs via the certstream websocket for suspicious domain registrations. It is designed for security researchers to detect phishing, typosquatting, and other malicious domains in real-time.

-----

## 2. Core Requirements

### 2.1. Data Source & Processing

  - **Input**: Connect to the certstream websocket server with automatic reconnection logic.
  - **Volume**: Handle a stream of 5,000-20,000 domains/second.
  - **Sampling**: Configurable sampling rate (0.00-1.00) to manage high-volume streams under resource constraints.
  - **Performance**: A fully non-blocking architecture using async processing for all I/O-bound operations.

### 2.2. Pattern Matching System

  - **Engine**: Must efficiently match against **thousands of concurrent regex rules**. The implementation will use a high-performance, multi-pattern matching engine (e.g., Rust's `regex::RegexSet`) that compiles all patterns into a single finite automaton.
  - **Pattern Sources**: Load regex patterns from multiple external files.
  - **Hot Reload**: Pattern files can be updated in real-time. The system will compile the new ruleset in the background and **perform a seamless switchover** without interrupting the data stream. A minor delay in new rule activation is acceptable.
  - **Tagging**: Each match must be tagged with a `source_tag` derived from the pattern file's name (e.g., a pattern from `phishing.txt` tags the match as `phishing`).

### 2.3. Deduplication

  - **Uniqueness**: An alert is considered unique based on a compact, fixed-size **hash** of a key.
  - **Standard Alert Key**: For most alerts, the key is the **`(domain_name, source_tag)`**.
  - **First Resolution Alert Key**: For the special alert when an `NXDOMAIN` domain resolves, the key is the **`(domain_name, source_tag, "resolved_after_nxdomain")`** to ensure it is never suppressed.
  - **Window**: A configurable rolling time window for preventing duplicate alerts, defaulting to **1 hour**.

### 2.4. DNS Resolution & Alerting

  - **Record Types**: The resolver will query for **A (IPv4), AAAA (IPv6), and NS** records. `CNAME` records will be ignored.
  - **Immediate `NXDOMAIN` Alert**: When a matched domain returns `NXDOMAIN` on its first lookup, the system will **immediately generate an alert**. This alert will contain the domain and tag, but the DNS and enrichment fields will be represented as empty arrays.
  - **Follow-up "First Resolution" Alert**: If a domain that previously returned `NXDOMAIN` successfully resolves via the retry mechanism, a **second, completely new alert will be generated**. This alert will be fully populated with all DNS/ASN data and will contain the `"resolved_after_nxdomain": true` flag.

### 2.5. DNS Resolution Retry Policy

  - **Dual-Curve Exponential Backoff**: The system will implement two separate retry strategies:
    1.  **Standard Failures (Timeouts, Server Errors):** Will use a configurable, more aggressive retry curve to quickly overcome transient network issues.
    2.  **NXDOMAIN Failures:** Will use a separate, less aggressive retry curve with longer delays between attempts to monitor for domain activation without creating excessive load.

### 2.6. IP Enrichment

  - **ASN Lookup**: Perform an ASN lookup for **every** IP address resolved from the `A` and `AAAA` records using a local TSV file. The implementation uses a high-performance in-memory interval map for lookups.

### 2.7. Output Formats

  - **JSON**: A structured, detailed JSON output suitable for file logging and integration.
    ```json
    // Standard alert
    {
      "timestamp": "...",
      "domain": "example.com",
      "source_tag": "phishing",
      "resolved_after_nxdomain": false, // Key field!
      "dns": { "a_records": ["1.1.1.1"], "aaaa_records": [], "ns_records": [] },
      "enrichment": [ { "ip": "1.1.1.1", ... } ]
    }

    // Follow-up resolution alert
    {
      "timestamp": "...",
      "domain": "newly-active.com",
      "source_tag": "phishing",
      "resolved_after_nxdomain": true, // Key field!
      "dns": { "a_records": ["2.2.2.2"], ... },
      "enrichment": [ { "ip": "2.2.2.2", ... } ]
    }
    ```
  - **stdout (Plain text)**: A **compact, single-line summary**. Format: `[tag] domain -> first_ip [country, as_number, as_name] (+n other IPs)`.
  - **Slack**: Notifications sent via webhook using the same compact, single-line summary format as `stdout`.

-----

## 3. Configuration Management

### 3.1. Command Line Options

  - `--dedup-window SECONDS`: Deduplication window duration (default: 3600).
  - `--sample-rate FLOAT`: Sampling percentage (0.00-1.00).
  - `--dns-timeout-ms MILLISECONDS`: DNS resolution timeout.
  - `--dns-resolver IP`: DNS resolver IP address.
  - `--live-metrics`: Real-time metrics display.

### 3.2. Config File Options

  - All command-line options.
  - Webhook configurations (Slack URLs, channels).
  - Pattern file paths and their associated `source_tag`.
  - Output destinations and formats.
  - **DNS Retry Policies**:
    ```yaml
    dns_retry:
      standard_retries: 3
      standard_initial_backoff_ms: 500
      nxdomain_retries: 5
      nxdomain_initial_backoff_ms: 10000
    ```

-----

## 4. Monitoring & Metrics

  - **Real-time Metrics**: An optional live display toggled by `--live-metrics`.
  - **Key Metrics**:
      - Domains processed/second
      - Pattern matches/second (per tag)
      - DNS resolution success/failure/timeout rate
      - `NXDOMAIN` retry queue size
      - Webhook delivery success/failure rate
      - Deduplication cache size
      - Memory and CPU usage

-----

## 5. Success Criteria

  - Process 5,000-20,000 domains/second efficiently on standard hardware.
  - Sub-second latency for pattern matching against thousands of rules.
  - Memory usage remains stable during extended operation.
  - Hot-reload of pattern files occurs with zero service interruption.
  - The dual-alert system reliably identifies and flags newly activated domains.
