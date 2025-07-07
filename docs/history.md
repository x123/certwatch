### Epic 33: GitHub Documentation Best Practices
**Goal:** Enhanced the project's `README.md` to align with GitHub best practices, creating a more welcoming and informative entry point for new contributors and users. The updated documentation clarifies the project's purpose, features, and setup, making it easier for the community to get involved.

---
### Epic 29: Resolve Metrics Crate Name Collision
**Goal:** Resolved a critical compilation issue by renaming the internal `metrics` module to `internal_metrics`, eliminating the name collision with the external `metrics` crate. This refactoring unblocked future development and ensured the stability of the metrics system.

---
### Epic 28: Robust Shutdown for DNS Resolution
**Goal:** Hardened the application's shutdown sequence by making the DNS resolution process shutdown-aware. The system can now interrupt in-flight DNS queries, allowing for a prompt and graceful exit, even under heavy load.

---
### Epic 27: Anti-Regression Testing for Graceful Shutdown
**Goal:** Implemented a timeout-based integration test to prevent shutdown hangs and deadlocks. This test runs the entire application and asserts that it terminates within a strict time limit, providing a critical safeguard against regressions.

---
### Epic 26: Robust Pattern Hot-Reloading
**Goal:** Re-engineered the pattern hot-reloading mechanism to be more reliable. The new implementation correctly handles file creations, deletions, and atomic renames by watching parent directories and using a debouncer to manage event storms, ensuring rule changes are applied consistently.

---
### Epic 25.5: Decouple Ingress from Processing to Fix Bottleneck
**Goal:** Rearchitected the application to use a fan-out worker pool, decoupling the high-speed network client from the slower, resource-intensive domain processing logic. This eliminated a critical performance bottleneck, allowing the system to process the full CertStream firehose without dropping messages.

---
### Epic 25: Correctness & Reliability Fixes
**Goal:** Addressed several critical bugs to improve application stability, including fixing the deduplication logic for "first resolution" alerts, ensuring atomic state updates in the DNS health monitor, and adding support for file deletion events in the pattern watcher.

---
### Epic 24: Performance & Stability Hardening
**Goal:** Replaced the "one task per domain" processing model with a bounded concurrency approach, significantly improving performance and stability under heavy load. This change prevents the application from being overwhelmed by high-volume data streams.

---
### Epic 23: Time-Windowed Log Aggregation
**Goal:** Implemented a log aggregation mechanism to consolidate noisy, high-frequency log messages into periodic summaries. This makes the logs easier to read and analyze, improving the operator experience.

---
### Epic 22: `trust-dns-resolver` Deprecation - Phase 2 (Idiomatic Refactor)
**Goal:** Refactored the DNS implementation to align with the idiomatic patterns of `hickory-resolver`, making the code more maintainable and leveraging the full capabilities of the new library.

---
### Epic 21: `trust-dns-resolver` Deprecation - Phase 1 (Minimal Migration)
**Goal:** Replaced the deprecated `trust-dns-resolver` with its successor, `hickory-resolver`, to ensure the project no longer depends on unmaintained code. The migration was performed with minimal code changes to reduce risk.

---
### Epic 20: Command-Line JSON Output
**Goal:** Added a `-j` / `--json` command-line flag to switch the standard output to JSON format, allowing for easy integration with other command-line tools like `jq`.

---
### Epic 19: Configurable DNS Resolver & Timeout
**Goal:** Implemented command-line and configuration file options to specify the DNS resolver and query timeout, allowing operators to adapt the tool to different network environments.

---
### Epic 18: Documentation & Codebase Alignment
**Goal:** Aligned the project's documentation with the current implementation, removing obsolete information and ensuring that the `README.md` and `docs/specs.md` accurately reflect the application's features and configuration.

---
### Epic 17: Improve Metrics Readability
**Goal:** Improved the clarity of the application's metrics by renaming the `deduplication_cache_size` metric to `deduplication_cache_entries` and formatting gauge values as integers, making them more intuitive for operators.

---
### Epic 16: Implement Command-Line Interface (CLI)
**Goal:** Integrated the `clap` crate to provide a robust command-line interface, allowing users to override settings from the `certwatch.toml` file and run the tool in different modes without editing files.

---
### Epic 15: Simplify ASN Enrichment with High-Performance TSV Provider
**Goal:** Replaced the binary `maxminddb` dependency with a more transparent and simpler TSV-based provider, using an interval tree for high-performance lookups. This simplified the build and made the enrichment data easier to manage.

---
### Epic 14: Dependency Maintenance
**Goal:** Updated the project's dependencies to their latest versions to incorporate security patches, performance improvements, and new features.

---
### Epic 12: Core Metrics with Logging
**Goal:** Instrumented the application with key performance and operational metrics using the `metrics` crate and implemented a logging recorder to provide a simple way to view these metrics.

---
### Epic 11: Stateful DNS Health Monitoring
**Goal:** Implemented a DNS health monitor to track the failure rate of DNS resolutions, providing a clear signal of systemic DNS problems while suppressing log spam during intermittent failures.

---
### Epic 10: Configurable Output Formats
**Goal:** Refactored the output system to support multiple, configurable formats for `stdout`, with a human-readable summary as the default.

---
### Epic 9: Unify Country Code Enrichment
**Goal:** Refactored the enrichment pipeline to handle country codes consistently across all providers, ensuring that the country code from the TSV provider is included in the final alert.

---
### Epic 8: Implement Configurable Sampling
**Goal:** Implemented a configurable sampling mechanism in the `CertStreamClient` to manage high-volume data streams and reduce resource usage.

---
### Epic 7: Alternative TSV-Based Enrichment Provider
**Goal:** Introduced a new, self-contained ASN enrichment provider that uses a local TSV file, reducing reliance on the MaxMind database format.

---
### Epic 6: Finalization & Integration
**Goal:** Tied all the decoupled components together into a running application, including configuration management and the wiring of all services in `main.rs`.

---
### Epic 5: Output & Alerting
**Goal:** Built a flexible output system for delivering alerts, including a deduplication service to filter alerts based on defined unique keys.

---
### Epic 4.5: Pre-Flight Refactoring
**Goal:** Addressed technical debt and feature gaps identified during a code review, including fixing the DNS resolution pipeline and adding GeoIP country enrichment.

---
### Epic 4: High-Performance Pattern Matching
**Goal:** Built the core detection engine with a focus on testability and hot-reloading, using `regex::RegexSet` for high performance.

---
### Epic 3: Data Ingestion
**Goal:** Built a robust, testable client for the `certstream` websocket, with a decoupled design that allows for simulating network interactions during tests.

---
### Epic 2: Data Ingestion
**Goal:** Built a robust, testable client for the `certstream` websocket.

---
### Epic Tech Debt 1: Improve Live Integration Testing
**Goal:** Paid down technical debt by refactoring duplicated live test setup code and adding new live integration tests for pattern hot-reloading and the enrichment pipeline.

---
### Epic 1: Project Setup & Core Plumbing
**Goal:** Established the project's foundation, focusing on a testable, decoupled structure from the start by defining core data structures and primary `trait` contracts.
