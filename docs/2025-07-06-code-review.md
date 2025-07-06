# Code Review: `certwatch` - 2025-07-06

## 1. Executive Summary

The `certwatch` codebase is a well-architected, high-performance application that faithfully implements the sophisticated requirements outlined in the project's specification documents. The use of modern, asynchronous Rust, a clear pipeline architecture, and high-quality libraries forms a strong foundation. The service-oriented design, based on `async_trait` traits, is a particular highlight, promoting testability and maintainability.

This review has identified one critical performance bottleneck, a significant correctness bug in the deduplication logic, and several areas where the code can be made more robust, efficient, and idiomatic. Addressing these findings will significantly improve the application's stability under load and align it more closely with its performance goals.

## 2. High-Level Architectural Findings

*   **Strengths**:
    *   **Pipeline Architecture**: The use of `tokio` tasks and `mpsc` channels to create a multi-stage processing pipeline is clean and effective.
    *   **Service-Oriented Design**: The use of `async_trait` to define service contracts (`DnsResolver`, `PatternMatcher`, etc.) is excellent and promotes loose coupling.
    *   **Configuration Management**: The configuration system using `figment` and `clap` is robust and flexible.
    *   **Advanced Features**: The implementation of complex features like `NXDOMAIN` dual-curve retries and hot-reloading of patterns is sophisticated and well-executed.

*   **Areas for Improvement**:
    *   **Concurrency Strategy**: The main processing loop's "one task per domain" approach is a critical performance bottleneck that needs to be replaced with a bounded concurrency model.
    *   **Error Handling and State Management**: Several modules have minor issues with race conditions, inconsistent state management, and vague error contracts that should be tightened.

## 3. Detailed Findings & Suggestions

### 3.1. Critical Issues

#### 3.1.1. Performance: Unbounded Task Spawning in `main`
*   **File**: [`src/main.rs:201-237`](src/main.rs:201)
*   **Problem**: The main processing loop spawns a new `tokio` task for every single domain. At the target rate of 5,000-20,000 domains/sec, this will cause extreme overhead from task creation and scheduling, leading to high CPU usage and instability.
*   **Suggestion**: Refactor the loop to use a bounded concurrency pattern. Replace the `tokio::spawn` per domain with `futures::stream::StreamExt::for_each_concurrent`, limiting the number of in-flight processing tasks to a configurable value (e.g., `num_cpus::get() * 2`).

#### 3.1.2. Bug: Incorrect Deduplication Key for "First Resolution" Alerts
*   **File**: [`src/deduplication.rs:48-53`](src/deduplication.rs:48)
*   **Problem**: The deduplication key for "first resolution" alerts is a constant string combined with the domain. This means all such alerts for the same domain will be treated as duplicates, breaking a critical feature.
*   **Suggestion**: The key must be unique per domain and source tag. Change the key generation to include the `source_tag`: `format!("{}:{}:FIRST_RESOLUTION", alert.domain, alert.source_tag)`.

### 3.2. Minor Issues & Refinements
### 3.3. Minor Issues & Refinements

#### 3.3.1. Configuration
*   **File**: [`Cargo.toml:4`](Cargo.toml:4)
*   **Issue**: The Rust edition is set to `2024`, which is invalid.
*   **Suggestion**: Change the edition to `2021`.

*   **File**: [`src/config.rs:128`](src/config.rs:128) and [`src/main.rs:131-133`](src/main.rs:131)
*   **Issue**: `enrichment.asn_tsv_path` is optional in the config but required at runtime, causing a panic if not present.
*   **Suggestion**: If enrichment is required, make `asn_tsv_path` non-optional in `EnrichmentConfig`. If it is optional, handle the `None` case in `main.rs` by substituting a "null" enrichment provider.

#### 3.3.2. DNS Module
*   **File**: [`src/dns/health.rs:36`](src/dns/health.rs:36)
*   **Issue**: `DnsHealthMonitor::new` returns an `Arc<Self>`, which is unconventional.
*   **Suggestion**: The constructor should return `Self`. The caller in `main.rs` should be responsible for wrapping it in an `Arc`.

*   **File**: [`src/dns/health.rs:111`](src/dns/health.rs:111)
*   **Issue**: Potential race condition in the health recovery logic.
*   **Suggestion**: The lock on `state` and `outcomes` should be held for the entire duration of the recovery state change to ensure atomicity.

*   **File**: [`src/dns.rs:295`](src/dns.rs:295)
*   **Issue**: The `nxdomain_retry_task` uses a `select!` loop with a manual sleep, which is less efficient and more complex than modern alternatives.
*   **Suggestion**: Replace the loop with `tokio::time::sleep_until` for a more efficient and idiomatic implementation.

#### 3.3.3. Matching Module
*   **File**: [`src/matching.rs:258`](src/matching.rs:258)
*   **Issue**: The file watcher does not handle `Remove` events for pattern files.
*   **Suggestion**: Update `should_reload_patterns` to also trigger on `Remove` events.

*   **File**: [`src/matching.rs:198`](src/matching.rs:198)
*   **Issue**: The file watcher callback uses a blocking send (`blocking_send`) in an async context.
*   **Suggestion**: Use a non-blocking `try_send` and log a warning if the channel is full, or ensure the channel has sufficient capacity.

#### 3.3.4. Outputs Module
*   **File**: [`src/outputs.rs:71`](src/outputs.rs:71)
*   **Issue**: The `StdoutOutput` uses blocking I/O (`std::io::stdout().lock()`) in an async task.
*   **Suggestion**: Use `tokio::io::stdout()` and `tokio::io::AsyncWriteExt` for non-blocking writes.

*   **File**: [`src/outputs.rs:92`](src/outputs.rs:92)
*   **Issue**: The `format_plain_text` function inefficiently builds a `HashMap` on every call.
*   **Suggestion**: Refactor the function to iterate over the `enrichment` `Vec` directly to find the required data, avoiding the unnecessary allocation.
