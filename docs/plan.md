### **Epic 1: Project Setup & Core Plumbing**
This phase establishes the project's foundation, focusing on a testable, decoupled structure from the start.

- [x] **#1 - Initialize Project & Define Core Traits**
  - **Context:** Set up the project, establish the module structure, and define the core data structures and the primary `trait` contracts that will govern component interactions. This ensures a decoupled architecture from the outset.
  - **Dependencies:** None
  - **Subtasks:**
    - [x] Initialize a new binary Rust project: `cargo new certwatch --bin`.
    - [x] Add core dependencies to `Cargo.toml`: `tokio`, `serde`, `anyhow`, `log`, `env_logger`, and dev-dependencies for testing.
    - [x] Create module files (`main.rs`, `core.rs`, `network.rs`, `matching.rs`, etc.).
    - [x] In `core.rs`, define the final data structs (`Alert`, `DnsInfo`, `AsnInfo`) using `serde`.
    - [x] In `core.rs`, define the primary service traits that will be injected as dependencies, e.g., `trait PatternMatcher`, `trait DnsResolver`, `trait AsnLookup`, `trait Output`.

---
### **Epic 2: Data Ingestion**
This phase builds a robust, testable client for the `certstream` websocket.

- [x] **#2 - Implement Testable CertStream Message Parsing**
  - **Context:** Following TDD, create a function that can reliably parse a raw `certstream` JSON string into our core domain objects. This logic will be tested in isolation before any network code is written.
  - **Dependencies:** #1
  - **Subtasks:**
    - [x] Write a unit test in `network.rs` that provides a sample JSON string and asserts that the parsing function correctly extracts a `Vec<String>` of domains or returns an appropriate `Err`.
    - [x] Implement the `parse_message(text: &str) -> Result<Vec<String>, Error>` function to make the test pass.

- [x] **#3 - Implement a Decoupled WebSocket Client**
  - **Context:** Build the WebSocket client. It will depend on a `WebSocket` trait for testability, allowing us to simulate network interactions without making real connections during tests.
  - **Dependencies:** #2
  - **Subtasks:**
    - [x] Write an integration test in `/tests` that uses a fake WebSocket implementation to test the client's reconnect logic.
    - [x] Define a `trait WebSocketConnection` with methods like `connect` and `read_message`.
    - [x] Implement the `CertStreamClient` struct, which takes a `Box<dyn WebSocketConnection>` as a dependency.
    - [x] Implement the client's connection and auto-reconnect logic, using channels (`tokio::sync::mpsc`) to send parsed domains to the next stage.

---
### **Epic 3: High-Performance Pattern Matching**
This phase builds the core detection engine with a focus on testability and hot-reloading.

- [x] **#4 - Implement the Pattern Matching Engine**
  - **Context:** Build the service that matches domains against thousands of regex rules. The implementation will be hidden behind the `PatternMatcher` trait defined in task #1.
  - **Dependencies:** #1
  - **Subtasks:**
    - [x] Write unit tests for the `PatternMatcher`. One test should assert a successful match returns the correct domain and tag. Another should assert no match returns `None`.
    - [x] Implement a `struct RegexMatcher` that fulfills the `trait PatternMatcher`.
    - [x] The implementation should use `regex::RegexSet` for high performance.
    - [x] The constructor will take a `Vec<(String, String)>` of `(pattern, source_tag)` to build its internal state.

- [x] **#5 - Implement Pattern Loading and Hot-Reload**
  - **Context:** Create the logic to load patterns from files and swap them into the `RegexMatcher` atomically and safely.
  - **Dependencies:** #4
  - **Subtasks:**
    - [x] Write a unit test to verify that the pattern loading function correctly reads a file and produces a `Vec` of `(pattern, tag)`.
    - [x] Implement the file loading and parsing logic.
    - [x] Write an integration test to verify the hot-reload mechanism. This test will write to a file and assert that the `PatternMatcher` starts matching new patterns after a short delay.
    - [x] Implement the hot-reload service using a file watcher (e.g., `notify`) that rebuilds the `RegexSet` in a background task and swaps it using an `ArcSwap`.

---
### **Epic 4: DNS Resolution & Enrichment**
This phase focuses on enriching domains with DNS/ASN data, using a fully testable, trait-based approach.

- [x] **#6 - Implement the DNS Resolver Service**
  - **Context:** Implement the `DnsResolver` trait. The implementation will handle A, AAAA, and NS lookups and the dual-curve retry logic, ensuring all network errors are propagated via `Result`.
  - **Dependencies:** #1, #3
  - **Subtasks:**
    - [x] Write unit tests for the retry logic. Use a hand-written `FakeDnsResolver` that implements the `DnsResolver` trait to simulate timeouts, server errors, and `NXDOMAIN` responses, asserting the retry behavior is correct.
    - [x] Implement a `struct TrustDnsResolver` that uses the `trust-dns-resolver` crate.
    - [x] Ensure all fallible operations within this struct return a `Result` and use the `?` operator for error propagation.
    - [x] Implement the logic for the `NXDOMAIN` retry queue and the `"resolved_after_nxdomain"` flag.

- [x] **#7 - Implement the ASN Enrichment Service**
  - **Context:** Build the IP-to-ASN enrichment service, hiding the database implementation behind the `AsnLookup` trait.
  - **Dependencies:** #1, #6
  - **Subtasks:**
    - [x] Write a unit test for the `AsnLookup` trait, providing a sample IP and asserting the correct `AsnInfo` is returned.
    - [x] Implement a `struct MaxmindAsnLookup` that implements the trait.
    - [x] The constructor for the struct will load the MaxMindDB file. The lookup method will handle cases where an IP is not found in the database by returning a `Result`.

---
### **Epic Tech Debt 1: Improve Live Integration Testing**
This epic focuses on paying down technical debt by leveraging our live testing capabilities to improve the robustness and maintainability of existing features.

- [x] **#7.1 - Refactor Duplicated Live Test Setup**
  - **Context:** Our live tests (`live_certstream_client.rs`, `live_pattern_matching.rs`) contain duplicated boilerplate for setting up the `CertStreamClient` and `tokio` runtime. This should be refactored into a reusable test harness.
  - **Dependencies:** #3
  - **Subtasks:**
    - [x] Create a test harness function in `tests/common.rs` that encapsulates the common setup and teardown logic for live tests.
    - [x] Refactor `live_certstream_client.rs` to use the new harness.
    - [x] Refactor `live_pattern_matching.rs` to use the new harness.

- [x] **#7.2 - Add Live Integration Test for Pattern Hot-Reload**
  - **Context:** The pattern hot-reloading mechanism is currently only validated by a unit test. A live test is needed to verify its correctness in a concurrent environment.
  - **Dependencies:** #5
  - **Subtasks:**
    - [x] Create a new test file `tests/live_hot_reload.rs`.
    - [x] The test will connect to the live feed, match against initial patterns, update the pattern file, and assert that the new patterns are loaded and used correctly.
    - [x] Remove obsolete, flaky hot-reload tests from `tests/pattern_hot_reload.rs`.

- [x] **#7.3 - Add Live Integration Test for Enrichment Pipeline**
  - **Context:** The DNS and ASN enrichment services are only validated with unit tests. A live integration test will provide higher confidence in their implementation.
  - **Dependencies:** #6, #7
  - **Subtasks:**
    - [x] Create a new test file `tests/live_enrichment.rs`.
    - [x] The test will pull domains from the live stream, resolve them using `TrustDnsResolver`, and enrich them with `MaxmindAsnLookup`.
    - [x] Assert that the pipeline runs without errors and successfully enriches a sample of domains.

---
### **Epic 4.5: Pre-Flight Refactoring**
This epic addresses technical debt and feature gaps identified during the 2025-07-05 code review before proceeding to the next major feature set.

- [x] **#A - Fix DNS Resolution Pipeline**
  - **Context:** The current DNS implementation has two critical flaws: the "First Resolution" alert for `NXDOMAIN` domains is not fully implemented, and the error handling in `TrustDnsResolver` swallows specific errors.
  - **Dependencies:** #6
  - **Subtasks:**
    - [x] Refactor the `nxdomain_retry_task` to send successfully resolved domains to the main application pipeline for alert generation.
    - [x] Correct the error handling in `TrustDnsResolver` to propagate specific DNS errors instead of replacing them with a generic message.
    - [x] Write unit tests for the `nxdomain_retry_task` logic.

- [x] **#B - Implement Configurable Sampling**
  - **Context:** The `CertStreamClient` lacks the required sampling feature to manage high-volume streams. (Superseded by Epic 8)
  - **Dependencies:** #3
  - **Subtasks:**
    - [x] Add a `sample_rate` field to the application's configuration.
    - [x] In `CertStreamClient`, implement logic to process only a percentage of incoming domains based on the `sample_rate`.

- [x] **#C - Add GeoIP Country Enrichment**
  - **Context:** The enrichment service currently uses a placeholder for the `country_code`. A real implementation is needed to match the spec.
  - **Dependencies:** #7
  - **Subtasks:**
    - [x] Add a `GeoIpLookup` trait and a `MaxmindGeoIpLookup` implementation that uses the `GeoLite2-Country.mmdb` database.
    - [x] Integrate the `GeoIpLookup` service into the main pipeline to add the country code to the `AsnInfo` struct.

- [x] **#D - General Code Cleanup**
  - **Context:** Address medium-priority cleanup tasks identified in the code review.
  - **Dependencies:** #3, #5
  - **Subtasks:**
    - [ ] Refactor the duplicated message handling logic in `network.rs`.
    - [ ] Replace `println!` calls in `matching.rs` with `log::info!`.
    - [ ] Simplify the `PatternWatcher` API to remove the confusing `HashMap` parameter.
    - [ ] Make the self-signed certificate handling in `network.rs` an explicit configuration option.

---
### **Epic 5: Output & Alerting**
This phase builds the flexible output system for delivering alerts.

- [x] **#8 - Implement the Output Management System**
  - **Context:** Build the `OutputManager` and the specific output formatters. The entire system will be built on the `Output` trait.
  - **Dependencies:** #1
  - **Subtasks:**
    - [x] Write unit tests for each output formatter (JSON, stdout, Slack). The tests for Slack will use a fake HTTP client injected as a dependency.
    - [x] Implement `JsonOutput`, `StdoutOutput`, and `SlackOutput` structs, each fulfilling the `trait Output`.
    - [x] Implement the `OutputManager` which holds a `Vec<Box<dyn Output>>` and dispatches alerts accordingly.

- [x] **#9 - Implement Deduplication**
  - **Context:** Build the deduplication service that filters alerts based on the defined unique keys.
  - **Dependencies:** #8
  - **Subtasks:**
    - [x] Write unit tests to verify the deduplication logic. Assert that a second alert with the same key is dropped, but an alert with the unique "First Resolution" key is processed.
    - [x] Implement the `Deduplicator` service using a time-aware cache (e.g., `moka`) and the specified hashing logic for keys.

---
### **Epic 6: Finalization & Integration**
This final phase ties all the decoupled components together into a running application.

- [x] **#10 - Implement Configuration Management**
  - **Context:** Load all application settings from a file and command-line arguments, making them available to the rest of the application.
  - **Dependencies:** #1
  - **Subtasks:**
    - [x] Define a single, comprehensive `Config` struct using `serde`.
    - [x] Write a unit test to verify that loading a sample TOML file correctly populates the `Config` struct.
    - [x] Implement the configuration loading logic using a crate like `figment` to merge file-based settings with CLI arguments.

- [x] **#11 - Wire All Components in `main.rs`**
  - **Context:** Write the main application function that initializes all concrete service implementations, injects them into the components that depend on their traits, and starts all concurrent tasks.
  - **Dependencies:** #3, #5, #7, #8, #9, #10
  - **Subtasks:**
    - [x] In `main`, load the configuration from task #10.
    - [x] Instantiate the concrete types: `RegexMatcher`, `TrustDnsResolver`, `MaxmindAsnLookup`, `OutputManager`, etc.
    - [x] Create the `mpsc` channels that will connect the pipeline stages.
    - [x] "Wire" the application by passing the concrete instances (as `Box<dyn Trait>`) into the constructors of the services that need them.
    - [x] Spawn a `tokio` task for each concurrent process (client, matcher, resolver pool, etc.) and start the runtime.


---
### **Epic 7: Alternative TSV-Based Enrichment Provider**
This epic introduces a new, self-contained ASN enrichment provider that uses a local TSV file, reducing reliance on the MaxMind database format.

- **#12 - Implement High-Performance TSV/CSV IP Lookup Engine**
  - **Context:** Build the core engine for parsing a TSV file of IP ranges and looking up addresses efficiently. Performance is critical, so the implementation must avoid linear scans.
  - **Dependencies:** #1
  - **Subtasks:**
    - [x] Create a new module `src/enrichment/tsv_lookup.rs`.
    - [x] Implement a parser for the tab-separated format (`start_ip`, `end_ip`, `asn`, `country`, `description`).
    - [x] Implement logic to convert both IPv4 and IPv6 address strings into `u128` integers for unified comparison.
    - [x] Create a data structure that stores `(start: u128, end: u128, data: AsnInfo)` records in a sorted `Vec`.
    - [x] Implement the lookup function using a binary search on the sorted `Vec` to achieve `O(log n)` performance.
    - [x] Write unit tests to verify the parser and the binary search lookup logic against the `ip2asn-combined-test.tsv` data.

- **#13 - Integrate TSV Provider into Application**
  - **Context:** Integrate the new lookup engine into the application as a selectable alternative to the MaxMind provider.
  - **Dependencies:** #7, #12
  - **Subtasks:**
    - [x] Create a `struct TsvAsnLookup` that implements the `trait AsnLookup`.
    - [x] In `config.rs`, add an `enum AsnProvider { Maxmind, Tsv }` and a corresponding field to the `Config` struct.
    - [x] Add a `tsv_path` field to the configuration to specify the location of the data file.
    - [x] In `main.rs`, add logic to read the configuration and instantiate the selected `AsnLookup` provider.
    - [x] Write an integration test in `/tests` that runs the full pipeline with the `TsvAsnLookup` provider enabled.


---
### **Epic 8: Implement Configurable Sampling**
This epic addresses the feature gap identified in the 2025-07-06 code review, implementing the configurable sampling required by the specification to manage high-volume data streams.

- **#14 - Implement Sampling in CertStreamClient**
  - **Context:** The `CertStreamClient` currently processes every message from the websocket. To handle high-volume feeds and reduce resource usage, a sampling mechanism must be added as specified in `docs/specs.md` §2.1.
  - **Dependencies:** #10 (Configuration Management)
  - **Subtasks:**
    - [x] In `src/config.rs`, ensure a `sample_rate: f64` field exists in the `Config` struct with a default value of `1.0`.
    - [x] In `src/network.rs`, modify the `CertStreamClient` to read the `sample_rate` from the configuration.
    - [x] In the message processing loop within `CertStreamClient`, add logic to process only a percentage of incoming messages. Use a random number generator to decide whether to keep or drop each message based on the `sample_rate`.
    - [x] Add a unit test to `src/network.rs` to verify the sampling logic. The test should simulate a stream of messages and assert that, on average, the correct percentage is processed.


---
### **Epic 9: Unify Country Code Enrichment**
This epic addresses a feature gap identified during testing where the country code from the TSV provider was not being included in the final alert. It refactors the enrichment pipeline to handle country codes consistently across all providers.

- **#15 - Refactor Enrichment Traits and Data Structures**
  - **Context:** The current pipeline uses separate traits for ASN and GeoIP lookups, which is inefficient now that the TSV provider can supply both. This task will unify them.
  - **Dependencies:** #7, #12
  - **Subtasks:**
    - [x] In `src/core.rs`, add `country_code: Option<String>` to the `AsnInfo` struct.
    - [x] In `src/enrichment.rs`, merge the `GeoIpLookup` trait's functionality into the `EnrichmentProvider` trait. The `lookup` method will now return a complete `AsnInfo` object, including the country code.
    - [x] Delete the now-redundant `GeoIpLookup` trait.

- **#16 - Update Enrichment Providers**
  - **Context:** Both the MaxMind and TSV providers must be updated to implement the new unified `EnrichmentProvider` trait.
  - **Dependencies:** #15
  - **Subtasks:**
    - [x] In `src/enrichment/tsv_lookup.rs`, update `TsvAsnLookup` to store and return the country code in the `AsnInfo` struct.
    - [x] In `src/enrichment.rs`, refactor `MaxmindAsnLookup` to accept both ASN and GeoIP database readers in its constructor.
    - [x] Update the `MaxmindAsnLookup::lookup` method to perform both lookups and return a single, complete `AsnInfo` object.

- **#17 - Update Application Integration**
  - **Context:** The main application logic needs to be updated to use the new, simplified enrichment pipeline.
  - **Dependencies:** #16
  - **Subtasks:**
    - [x] In `main.rs`, update the logic to instantiate a single, unified `EnrichmentProvider`.
    - [x] Remove all logic related to the old `GeoIpLookup` service from the enrichment stage.
    - [x] Update integration tests, including `tests/live_enrichment.rs`, to assert that the `country_code` is present in the final alert output.


---
### **Epic 10: Configurable Output Formats**
This epic refactors the output system to support multiple, configurable formats for `stdout`, with a human-readable summary as the default, per the project specifications.

- **#18 - Update Configuration Model**
  - **Context:** The current configuration in `src/config.rs` only has a boolean `stdout_enabled`. This needs to be more expressive to allow format selection.
  - **Dependencies:** #10
  - **Subtasks:**
    - [x] In `src/config.rs`, create a new `OutputFormat` enum with `Json` and `PlainText` variants.
    - [x] In the `OutputConfig` struct, replace `stdout_enabled: bool` with a new `format: OutputFormat` field.
    - [x] Set the default value for `format` to `PlainText` in the `impl Default for Config`.

- **#19 - Update `certwatch.toml` Configuration File**
  - **Context:** The `certwatch.toml` file needs to be updated to reflect the new configuration options.
  - **Dependencies:** #18
  - **Subtasks:**
    - [x] In `certwatch.toml`, remove the `stdout_enabled` key.
    - [x] Add a new `format` key to the `[output]` section, e.g., `format = "PlainText"`.

- **#20 - Implement Plain Text Formatter**
  - **Context:** A new function is needed in `src/outputs.rs` to generate the human-readable summary string from an `Alert` object.
  - **Dependencies:** #18
  - **Subtasks:**
    - [x] In `src/outputs.rs`, create a new `format_plain_text(&Alert) -> String` function.
    - [x] Implement the formatting logic: `[tag] domain -> first_ip [country, as_number, as_name] (+n other IPs)`.
    - [x] Ensure the function correctly handles edge cases, such as alerts with no IP addresses or missing enrichment data.

- **#21 - Update Output Logic**
  - **Context:** The `StdoutOutput` struct in `src/outputs.rs` must be modified to use the new formatter.
  - **Dependencies:** #20
  - **Subtasks:**
    - [x] Modify `StdoutOutput` to store the configured `OutputFormat`.
    - [x] In the `send_alert` method, use a `match` statement on the format. If `Json`, serialize the alert as before. If `PlainText`, call the new `format_plain_text` function and print the resulting string.

- **#22 - Update Application Integration**
  - **Context:** The main application logic in `main.rs` needs to be updated to wire up the new configuration.
  - **Dependencies:** #21
  - **Subtasks:**
    - [x] In `main.rs`, when initializing the `OutputManager`, read the `output.format` from the configuration.
    - [x] Pass the configured format into the `StdoutOutput` constructor.

- **#23 - Update Tests**
  - **Context:** The test suite must be updated to validate the new functionality and prevent regressions.
  - **Dependencies:** #21
  - **Subtasks:**
    - [x] In `src/outputs.rs`, add unit tests specifically for the `format_plain_text` function, covering various alert types.
    - [x] Update the existing `StdoutOutput` tests to verify that both `Json` and `PlainText` formats work correctly based on the configuration.

---
### Epic 11: Stateful DNS Health Monitoring

**User Story:** As an operator, I want the system to be resilient to intermittent DNS failures, logging them as warnings. However, I want to be clearly alerted with an error if DNS resolution appears to be systemically failing, suggesting a problem with the resolver itself.

---

- **#24 - Implement DNS Health Monitor**
  - **Context:** Create a dedicated health monitoring component that wraps the DNS resolver. It will track the failure rate over a configurable time window and manage the resolver's state (`Healthy`, `Unhealthy`) to provide a clear signal of systemic DNS problems.
  - **Dependencies:** #6 (DNS Resolver Service)
  - **Subtasks:**
    - [x] In a new module `src/dns/health.rs`, define a `DnsHealthMonitor` struct.
    - [x] The monitor will maintain internal state: a deque of recent resolution outcomes (success/failure timestamps) and the current `HealthState` enum (`Healthy`, `Unhealthy`).
    - [x] Implement a `check_health()` method that is called on every resolution attempt. This method will record the outcome and recalculate the failure rate over the configured time window.
    - [x] If the failure rate exceeds the configured threshold, transition the state to `Unhealthy` and log a single, clear `ERROR` message.
    - [x] While `Unhealthy`, subsequent individual DNS failures should be logged at a `DEBUG` level to prevent log spam.
    - [x] Implement a background recovery check task. While `Unhealthy`, this task will periodically attempt to resolve a known-good domain (e.g., `google.com`).
    - [x] If the recovery check succeeds, transition the state back to `Healthy` and log an `INFO` message.

- **#25 - Integrate Health Monitor into Application**
  - **Context:** Wire the new `DnsHealthMonitor` into the main application pipeline.
  - **Dependencies:** #24
  - **Subtasks:**
    - [x] In `src/config.rs`, add a `DnsHealthConfig` struct with fields for `failure_threshold` (e.g., 0.95), `window_seconds` (e.g., 120), and `recovery_check_domain` (e.g., "google.com").
    - [x] In `src/dns.rs`, modify `DnsResolutionManager` to take an `Arc<DnsHealthMonitor>` as a dependency.
    - [x] In `resolve_with_retry`, call the health monitor's `check_health()` method after each resolution attempt.
    - [x] In `main.rs`, instantiate the `DnsHealthMonitor` and inject it into the `DnsResolutionManager`.
    - [x] Modify the main processing loop in `main.rs` to change the log level for individual DNS failures from `ERROR` to `WARN`. The health monitor will be responsible for escalating to `ERROR`.

- **#26 - Add Tests for Health Monitor**
  - **Context:** Add comprehensive tests to validate the health monitor's state transitions and behavior.
  - **Dependencies:** #24
  - **Subtasks:**
    - [x] In `src/dns/health.rs`, write unit tests to verify the state transition logic.
    - [x] Test that the state moves to `Unhealthy` when the failure threshold is breached.
    - [x] Test that the state moves back to `Healthy` after a successful recovery check.
    - [x] Test that log spam is suppressed while in the `Unhealthy` state.


---
### Epic 12: Core Metrics with Logging

**User Story:** As a developer, I want to instrument the application with key performance and operational metrics, and I want a simple way to view these metrics via logging so I can verify the instrumentation is working correctly before building a complex UI.

---

- [x] **#27 - Setup & Instrument Application**
  - **Context:** Add the core `metrics` dependency and sprinkle `metrics::*` macros throughout the application codebase at key locations to generate metric events.
  - **Dependencies:** All previous epics.
  - **Subtasks:**
    - [x] In `Cargo.toml`, add the `metrics` and `metrics-util` crates.
    - [x] In `src/network.rs`, add `metrics::counter!("domains_processed").increment(1);` for each domain received.
    - [x] In `src/matching.rs`, add `metrics::counter!("pattern_matches", "tag" => tag.clone()).increment(1);` for each match.
    - [x] In `src/dns.rs`, add counters for DNS resolution outcomes (`success`, `failure`, `timeout`).
    - [x] In `src/dns.rs`, add `metrics::gauge!("nxdomain_retry_queue_size").set(size);` to track the retry queue.
    - [x] In `src/outputs.rs`, add counters for webhook delivery outcomes.
    - [x] In `src/deduplication.rs`, add `metrics::gauge!("deduplication_cache_size").set(size);` to track the cache size.

- [x] **#28 - Implement Logging Recorder**
  - **Context:** Create a simple metrics recorder that captures metric events and periodically prints a summary to the standard log output (`log::info!`). This provides a verifiable way to see that the instrumentation is working.
  - **Dependencies:** #27
  - **Subtasks:**
    - [x] Create a new module `src/metrics/logging_recorder.rs`.
    - [x] Define a `LoggingRecorder` struct that implements the `metrics::Recorder` trait.
    - [x] The recorder will store metric state in a thread-safe container (e.g., `Arc<DashMap<...>>`).
    - [x] Spawn a background task that, every N seconds, iterates through the stored metrics and prints them using `log::info!`.

- [x] **#29 - Integrate Logging Recorder**
  - **Context:** Add a command-line flag to enable the `LoggingRecorder`, allowing for easy debugging and verification of the metrics system.
  - **Dependencies:** #28
  - **Subtasks:**
    - [x] In `src/config.rs`, add a `--log-metrics` command-line argument.
    - [x] In `main.rs`, check for the `--log-metrics` flag at startup.
    - [x] If `true`, instantiate the `LoggingRecorder` and install it as the global metrics recorder using `metrics::set_global_recorder`.
    - [x] If `false`, do nothing, ensuring zero performance overhead.

---
### Epic 13: Real-time Metrics TUI

**User Story:** As an operator, I want to see a real-time, terminal-based dashboard of key application metrics when I run the application with a specific flag, so that I can monitor its health and performance live.

---

- **#30 - Setup & Configuration**
  - **Context:** Add the dependencies for the TUI and expose a command-line flag to enable it.
  - **Dependencies:** #10 (Configuration Management)
  - **Subtasks:**
    - [ ] In `Cargo.toml`, add the `ratatui` and `crossterm` crates.
    - [ ] In `src/config.rs`, add a `--live-metrics` command-line argument.

- **#31 - Implement TUI Metrics Recorder**
  - **Context:** Create a custom recorder that implements the `metrics::Recorder` trait. This recorder will be responsible for capturing metric events and storing them in a thread-safe state that the TUI can access for rendering.
  - **Dependencies:** #28 (leverages similar state-holding patterns), #30
  - **Subtasks:**
    - [ ] Create a new module `src/metrics/tui_recorder.rs`.
    - [ ] Define a `TuiRecorder` struct that holds metric state in a thread-safe container, similar to the `LoggingRecorder`.
    - [ ] Implement the `metrics::Recorder` trait for `TuiRecorder`.

- **#32 - Implement Ratatui Frontend**
  - **Context:** Build the `ratatui` user interface that will run in its own thread, periodically reading data from the `TuiRecorder` and rendering it to the terminal.
  - **Dependencies:** #31
  - **Subtasks:**
    - [ ] Create a new module `src/metrics/ui.rs`.
    - [ ] Implement a `Tui` struct responsible for managing the terminal state (setup, drawing, and shutdown).
    - [ ] Create a `run_ui` function that takes a reference to the recorder's state. This function will contain the main render loop.
    - [ ] Inside the loop, fetch a snapshot of the metrics from the recorder's state.
    - [ ] Design and implement `ratatui` widgets to display the key metrics listed in `docs/specs.md` §4.
    - [ ] Ensure graceful shutdown and terminal restoration on exit (e.g., Ctrl+C).

- **#33 - Integrate TUI System in `main.rs`**
  - **Context:** Wire everything together. Based on the `--live-metrics` flag, install the custom TUI recorder and spawn the UI thread.
  - **Dependencies:** #32
  - **Subtasks:**
    - [ ] In `main.rs`, check the value of the `live_metrics` config flag at startup.
    - [ ] If `true`, instantiate the `TuiRecorder`, install it as the global recorder, and spawn a new thread to run the UI.
    - [ ] Ensure this can work alongside the `--log-metrics` flag, though typically only one would be used at a time.

---
### **Epic 14: Dependency Maintenance**

**User Story:** As a developer, I want to keep the project's dependencies up-to-date to benefit from the latest security patches, performance improvements, and features.

---

- **#34 - Update Low-Risk Dependencies**
 - **Context:** Update minor-version dependencies that have a low risk of introducing breaking changes.
 - **Dependencies:** All previous epics.
 - **Subtasks:**
   - [x] Update `env_logger` from `0.10` to `0.11.8`.
   - [x] Update `rand` from `0.8` to `0.9.1` and refactor usage of `thread_rng` and `gen`.
   - [x] Update `ipnetwork` from `0.20` to `0.21.1`, enabling the `serde` feature.
   - [x] Update `maxminddb` from `0.24` to `0.26.0` and refactor error handling.

- [x] **#35 - Update Network Client Dependencies**
  - **Context:** Update the networking clients, which are more likely to have breaking changes.
  - **Dependencies:** #34
  - **Subtasks:**
    - [x] Update `reqwest` from `0.11.27` to `0.12.22`.
    - [x] Update `tokio-tungstenite` from `0.20.1` to `0.27`.
    - [x] Run all tests, including live tests, to ensure no regressions.

- [x] **#36 - Update High-Risk `notify` Dependency**
  - **Context:** Update the `notify` crate, which has a major version bump and is likely to have significant breaking changes.
  - **Dependencies:** #35
  - **Subtasks:**
    - [x] Update `notify` from `6.1.1` to `8.1.0`.
    - [x] Refactor the `PatternWatcher` to be compatible with the new version.
    - [x] Run all tests, including live tests, to ensure no regressions.

---
### **Epic 15: Simplify ASN Enrichment with High-Performance TSV Provider**
This epic replaces the binary `maxminddb` dependency with a more transparent and simpler TSV-based provider, using an interval tree for high-performance lookups. This removes a complex dependency, simplifies the build, and makes the enrichment data easier to manage.

- [x] **#37 - Remove MaxMind Dependency and Implementation**
  - **Context:** Completely remove all code, configuration, and dependencies related to the `MaxmindEnrichmentProvider`.
  - **Dependencies:** None
  - **Subtasks:**
    - [x] In `Cargo.toml`, remove the `maxminddb` dependency.
    - [x] In `src/config.rs`, remove the `Maxmind` variant from the `AsnProvider` enum and delete the `asn_db_path` and `geoip_db_path` fields.
    - [x] In `certwatch.toml`, remove the configuration examples for `asn_db_path` and `geoip_db_path`.
    - [x] In `src/main.rs`, delete the logic block for initializing the `MaxmindEnrichmentProvider`.
    - [x] In `src/enrichment.rs`, delete the `MaxmindEnrichmentProvider` struct and its `impl`.
    - [x] Delete the `tests/live_enrichment.rs` file.
    - [x] Delete the `tests/enrichment_integration.rs` file.
    - [x] Delete the `tests/data/GeoLite2-ASN-Test.mmdb` and `tests/data/GeoLite2-Country-Test.mmdb` files.

- [x] **#38 - Implement Performance-Optimized TSV Provider**
  - **Context:** Create a new `TsvAsnLookup` that uses an interval tree for fast IP-to-ASN lookups from a TSV file.
  - **Dependencies:** #37
  - **Subtasks:**
    - [x] In `Cargo.toml`, add the `csv` and `rangetree` (or a similar interval tree) crates.
    - [x] In `src/enrichment/tsv_lookup.rs`, implement the `TsvAsnLookup` struct.
    - [x] The `TsvAsnLookup::new()` constructor will parse a TSV file (`CIDR\tAS_Number\tAS_Name`) and build an interval tree from the CIDR ranges.
    - [x] The `lookup()` method will perform a fast query against the interval tree.
    - [x] Update `src/main.rs` to initialize the `TsvAsnLookup` provider.

- [x] **#39 - Create New Tests for TSV Provider**
  - **Context:** Add comprehensive tests for the new TSV-based enrichment provider.
  - **Dependencies:** #38
  - **Subtasks:**
    - [x] Create a new test data file `tests/data/ip-to-asn-test.tsv`.
    - [x] Create a new integration test file `tests/tsv_enrichment.rs` to validate the parsing and lookup logic against the test TSV file.
    - [x] Ensure all tests pass, including running `cargo test --all-features`.

- [x] **#40 - Update Documentation**
  - **Context:** Update all relevant documentation to reflect the removal of MaxMind and the new TSV-based implementation.
  - **Dependencies:** #38
  - **Subtasks:**
    - [x] Review and update `docs/specs.md` to remove any references to the MaxMind database and describe the new TSV file format and configuration.
    - [x] Update the main `README.md` if it contains any setup instructions related to MaxMind.


---

### Epic 16: Implement Command-Line Interface (CLI)
- **User Story:** As a user, I want to configure the application using command-line arguments so that I can easily override the default settings from the `certwatch.toml` file and run the tool in different modes without editing files.
- **Tasks:**
  - [x] **#41: Add `clap` dependency:** Integrate the `clap` crate for robust CLI argument parsing.
  - [x] **#42: Create CLI argument struct:** Define a struct that derives `clap::Parser` to represent all command-line options specified in `docs/specs.md`.
  - [x] **#43: Integrate CLI with `figment`:** Update the configuration loading logic in `main.rs` to merge arguments parsed by `clap` with the configuration from `certwatch.toml` and environment variables. CLI arguments must have the highest priority.
  - [x] **#44: Add `--config` option:** Implement a dedicated `--config` or `-c` flag to allow users to specify a path to a different configuration file.
  - [x] **#45: Verify help message:** Ensure that running `cargo run -- --help` displays a well-formatted and comprehensive help message generated by `clap`.


---

### Epic 17: Improve Metrics Readability

**User Story:** As an operator, I want to see metrics that are intuitively named and formatted so that I can quickly understand the state of the application.

**Tasks:**
  - [x] **#46: Rename cache metric for clarity:** Change the `deduplication_cache_size` metric to `deduplication_cache_entries` to accurately reflect that it counts the number of items in the cache, not its memory footprint.
  - [x] **#47: Fix gauge number formatting:** Modify the logging recorder to format `f64` gauge values as integers to prevent confusing floating-point representations in the logs.


---

### Epic 18: Documentation & Codebase Alignment

**User Story:** As a developer, I want the project's documentation to accurately reflect the current state of the implementation so that I can understand its features and configuration without confusion.

**Tasks:**
  - [x] **#48: Align Metrics Documentation:**
    - In `README.md` and `docs/specs.md`, remove all references to the "Live Metrics TUI" and the `--live-metrics` flag.
    - Document the existing `--log-metrics` flag and the `LoggingRecorder` as the current method for viewing metrics.
    - In `src/cli.rs`, rename the `--live-metrics` flag to `--log-metrics` to match its actual function.
  - [x] **#49: Remove Obsolete DNS Configuration:**
    - In `src/config.rs`, remove the `resolver_pool_size` field from the `DnsConfig` struct.
    - Update `certwatch.toml` to remove the corresponding setting.
  - [x] **#50: Clean Up `plan.md`:**
    - Mark the redundant task `#B - Implement Configurable Sampling` in Epic 4.5 as complete, noting it was superseded by Epic 8.
    - Review other tasks in Epic 4.5 and mark them as complete or create new tasks if necessary.

---

### Epic 19: Configurable DNS Resolver & Timeout

**User Story:** As an operator, I want to specify which DNS resolver to use and control the query timeout via the config file or command line, so I can adapt the tool to different network environments.

**Tasks:**
  - [x] **#51: Implement DNS Query Timeout:**
    - In `src/config.rs`, add a `dns_timeout_ms: u64` field to `DnsConfig`.
    - In `src/cli.rs`, ensure the `--dns-timeout-ms` flag correctly maps to this new config field.
    - In `src/dns.rs`, modify `TrustDnsResolver` to accept the timeout and use it to configure the `ResolverOpts` of the underlying resolver. This timeout will apply to each individual query before the retry logic is triggered.
    - In `main.rs`, pass the configured timeout when creating the `TrustDnsResolver`.
  - [x] **#52: Implement Selectable DNS Resolver:**
    - In `src/config.rs`, add a `dns_resolver: Option<String>` field to `DnsConfig`.
    - In `src/cli.rs`, ensure the `--dns-resolver` flag maps to this field.
    - In `src/dns.rs`, modify `TrustDnsResolver` to accept an optional resolver IP. If provided, it will configure `trust-dns-resolver` to use that specific nameserver.
    - In `main.rs`, pass the configured resolver when creating the `TrustDnsResolver`.
  - [x] **#53: Update DNS Documentation:**
    - Update `docs/specs.md` and `README.md` to accurately describe the functionality of the `--dns-resolver` and `--dns-timeout-ms` options.
  - [x] **#53.1: Log active configuration:**
    - Log the active DNS resolver, timeout, and other important settings at application startup to improve operational visibility.

---

### Epic 20: Command-Line JSON Output

**User Story:** As a user, I want to switch the standard output to JSON format using a CLI flag, so I can easily pipe the tool's output to other programs like `jq`.

**Tasks:**
  - [x] **#54: Add JSON Output CLI Flag:**
    - In `src/cli.rs`, add a new boolean flag: `-j, --json`.
  - [x] **#55: Implement CLI Override for Output Format:**
    - In `main.rs`, when initializing `StdoutOutput`, check if the `--json` flag was passed. If true, override the format from the config file and use `OutputFormat::Json`.
  - [x] **#56: Update Output Documentation:**
    - Update `docs/specs.md` and `README.md` to describe the new `-j / --json` flag for switching `stdout` to JSON format.


---

### Epic 21: `trust-dns-resolver` Deprecation - Phase 1 (Minimal Migration)

**User Story:** As a developer, I want to replace the deprecated `trust-dns-resolver` with its successor, `hickory-resolver`, with minimal code changes, so that the project no longer depends on unmaintained code.

**Tasks:**
  - [x] **#57: Update Dependencies:**
    - In `Cargo.toml`, replace the `trust-dns-resolver` dependency with `hickory-resolver`.
    - Map the existing feature flags to their new equivalents in `hickory-resolver`.
  - [x] **#58: Update Codebase:**
    - Perform a global search-and-replace for `trust_dns_resolver` to `hickory_resolver` in all `use` statements.
    - Adapt the resolver creation in `src/dns.rs` to use the new builder pattern (`Resolver::builder()`) required by `hickory-resolver`.
  - [x] **#59: Validate Migration:**
    - Run the full test suite (`cargo test --all-features`) to ensure no regressions were introduced.
    - Manually run the application to confirm DNS resolution is still working as expected.

---

### Epic 22: `trust-dns-resolver` Deprecation - Phase 2 (Idiomatic Refactor)

**User Story:** As a developer, I want to refactor the DNS implementation to align with the idiomatic patterns of `hickory-resolver` so that the code is more maintainable and leverages the full capabilities of the new library.

**Tasks:**
  - [x] **#60: Refactor DNS Implementation:**
    - In `src/dns.rs`, rewrite the resolver creation and lookup logic to fully adopt the `hickory-resolver`'s builder patterns and configuration options.
  - [x] **#61: Review and Update Configuration:**
    - In `src/config.rs`, review the `DnsConfig` struct and update it to better align with the configuration options available in `hickory-resolver`.
  - [x] **#62: Enhance Error Handling:**
    - Adapt the error handling logic to use the `hickory_resolver::ResolveError` type, ensuring all relevant error variants are handled correctly.
  - [x] **#63: Update Tests:**
    - Update unit and integration tests to reflect the refactored implementation.

---

### Epic 23: Time-Windowed Log Aggregation

**User Story:** As an operator, I want to noisy, high-frequency log messages to be consolidated into periodic summaries, so that I can easily read the logs without being overwhelmed by repetitive information.

**Tasks:**
  - [x] **#64: Enhance Metrics Recorder for Aggregation:**
    - In `src/metrics/logging_recorder.rs`, modify the `LoggingRecorder` to support aggregation of specific metrics over a time window.
    - The recorder will need to distinguish between metrics that should be logged instantly and those that should be aggregated. This could be done via a naming convention (e.g., metrics starting with `agg.`) or explicit configuration.
  - [x] **#65: Implement Aggregation Logic:**
    - The `LoggingRecorder`'s background task will, for aggregatable metrics, sum their values over the logging interval.
    - After logging the summary (e.g., `log::debug!("Sent 322 domains to output channel in the last 5s")`), it will reset the counter for that metric.
  - [x] **#66: Update Network Module Logging:**
    - In `src/network.rs`, replace the direct `log::debug!` call with a metrics counter (e.g., `metrics::counter!("agg.domains_sent_to_output").increment(domains.len() as u64)`).
  - [x] **#67: Add Configuration:**
    - In `src/config.rs`, add a configuration section for the logging recorder to control the aggregation window (e.g., `log_aggregation_seconds: 5`).
  - [x] **#68: Update Tests:**
    - Add a unit test to verify that the aggregation logic works as expected. This may require a `FakeRecorder` or similar test harness to inspect the aggregated values over time.

---
### Epic 24: Performance & Stability Hardening
**User Story:** As an operator, I want the application to be stable and performant under heavy load, so that it can reliably process high-volume data streams without crashing or slowing down.

- [x] **#69 - Implement Bounded Concurrency for Domain Processing**
  - **Context:** The current "one task per domain" approach in the main processing loop is a critical performance bottleneck. This task will replace it with a bounded concurrency model to control the number of in-flight tasks and reduce overhead.
  - **Dependencies:** #11
  - **Subtasks:**
    - [x] In `main.rs`, refactor the main domain processing loop.
    - [x] Replace the `tokio::spawn` call for each domain with a `futures::stream::StreamExt::for_each_concurrent` block.
    - [x] Make the concurrency limit configurable, defaulting to a sensible value (e.g., `num_cpus::get() * 2`).
    - [x] Add a unit test to verify that the new loop processes domains correctly.

---
### Epic 25: Correctness & Reliability Fixes
**User Story:** As a developer, I want the application to be correct and reliable, so that it behaves as specified and is resilient to edge cases and unexpected inputs.

- [x] **#70 - Correct Deduplication Key for "First Resolution" Alerts**
  - **Context:** The deduplication logic for "first resolution" alerts is currently incorrect, causing all such alerts to be treated as duplicates. This task will fix the key generation to ensure correctness.
  - **Dependencies:** #9
  - **Subtasks:**
    - [ ] In `src/deduplication.rs`, modify the `generate_key` function.
    - [ ] Change the key for `resolved_after_nxdomain` alerts to include both the domain and the source tag.
    - [ ] Update the corresponding unit test to assert that "first resolution" alerts for different source tags are not treated as duplicates.

- [x] **#71 - Ensure Atomic State Updates in DNS Health Recovery**
  - **Context:** The DNS health recovery logic has a potential race condition. This task will make the state transition atomic.
  - **Dependencies:** #24
  - **Subtasks:**
    - [ ] In `src/dns/health.rs`, refactor the `recovery_check_task`.
    - [ ] Ensure the mutex lock is held for the entire duration of the state change (from `Unhealthy` to `Healthy`) and the clearing of outcomes.
    - [ ] Add a comment explaining why the lock is held to ensure atomicity.

- [ ] **#72 - Handle File Deletion Events in Pattern Watcher**
  - **Context:** The file watcher for pattern hot-reloading does not handle file deletions. This task will add support for `Remove` events.
  - **Dependencies:** #5
  - **Subtasks:**
    - [ ] In `src/matching.rs`, update the `should_reload_patterns` function to also trigger a reload on `notify::EventKind::Remove`.
    - [ ] Add a unit test to verify that deleting a pattern file correctly triggers a reload and removes the associated patterns.

---
### Epic 25.5: Decouple Ingress from Processing to Fix Bottleneck
**User Story:** As a security operator, I want the application to process every domain from the CertStream source without dropping messages, so that I have maximum visibility and do not miss potential threats due to internal performance bottlenecks.
**Technical Goal:** Refactor the application to use a fan-out worker pool architecture. This will decouple the high-speed network client from the slower, resource-intensive domain processing logic, eliminating the current performance bottleneck and allowing the system to process the full volume of the CertStream firehose.

- [x] **#73 - Introduce Central Domain Queue**
  - **Action:** In `main.rs`, create a new `mpsc` channel specifically for queuing individual `String` domains.
  - **Details:** This channel will act as the central work buffer between the network client and the processing workers. It should be configured with a large capacity (e.g., 100,000) to handle traffic bursts.

- [x] **#74 - Refactor `CertStreamClient` for Fan-Out**
  - **Action:** Modify the `CertStreamClient` in `src/network.rs` to push individual domains into the new central queue.
  - **Details:**
    - [x] Update `CertStreamClient::new` to accept a `Sender<String>`.
    - [x] In `handle_message`, iterate through the parsed `Vec<String>` of domains.
    - [x] For each domain, use a non-blocking `try_send` to place it onto the central queue.
    - [x] If `try_send` fails (meaning the queue is full), increment a "dropped_domains" metric and log a warning. This prevents the network client from ever blocking on a full channel.

- [x] **#75 - Implement the Worker Pool**
  - **Action:** In `main.rs`, replace the existing single processing loop with a pool of worker tasks.
  - **Details:**
    - [x] Spawn a number of asynchronous tasks equal to the `concurrency` setting from `certwatch.toml`.
    - [x] Each worker task will be given a clone of the `Receiver` end of the central domain queue.

- [x] **#76 - Adapt Worker Logic for Single-Domain Processing**
  - **Action:** The core logic currently in the `main` processing loop will be moved inside each worker task.
  - **Details:** Each worker will loop, receiving a single domain from the queue and then performing the full sequence of operations on it: pattern matching, DNS resolution, enrichment, and alert generation.

- [x] **#76.1 - Implement Ordered Graceful Shutdown**
  - **Action:** Refactor the shutdown logic to prevent race conditions and ensure clean termination.
  - **Details:**
    - [x] Implement a cascading shutdown sequence where the `CertStreamClient` termination closes the domain channel, which in turn signals workers to exit.
    - [x] The `output_task` and `nxdomain_feedback_task` now terminate naturally when their upstream channels close, preventing "channel closed" errors.
    - [x] This resolves a critical deadlock that prevented the application from exiting cleanly with Ctrl-C.

- [x] **#77 - Add Configuration and Metrics for the New Queue**
  - **Action:** Add a new configuration option and a metric to monitor the new architecture.
  - **Details:**
    - [x] Add `queue_capacity` to `certwatch.toml` under a new `[performance]` or similar section.
    - [x] Create a new `Gauge` metric named `domain_queue_fill_ratio` to monitor how full the queue is.
    - [x] Create a new `Counter` metric named `dropped_domains` to track domains lost due to a full queue.

---
### Epic 27: Anti-Regression Testing for Graceful Shutdown
**User Story:** As a developer, I want a robust suite of automated tests that can reliably detect shutdown hangs and deadlocks, so that this entire class of bug can be prevented from recurring in the future.

- [ ] **#85 - Implement Timeout-Based Shutdown Test**
  - **Context:** Create a high-level integration test that runs the entire application with mock inputs and asserts that it terminates within a strict time limit after a shutdown signal is sent. This acts as a primary, unambiguous signal for a deadlock.
  - **Dependencies:** #76.1
  - **Subtasks:**
    - [ ] Create a new test file `tests/shutdown_integration.rs`.
    - [ ] In the test, spawn the full application task using a helper function.
    - [ ] After a brief delay, send the shutdown signal.
    - [ ] `await` the application task's handle, wrapped in a `tokio::time::timeout`.
    - [ ] The test fails if the timeout is exceeded.

- [ ] **#86 - Implement Heartbeat-Monitoring Shutdown Test**
  - **Context:** Create a more precise test that leverages the existing heartbeat utility to identify which specific component fails to terminate. This provides immediate diagnostic information when the timeout test fails.
  - **Dependencies:** #85
  - **Subtasks:**
    - [ ] Add a test-specific logging subscriber (e.g., `tracing-subscriber::test`) to capture log output.
    - [ ] In a new test, run the application and capture logs.
    - [ ] After sending the shutdown signal, continue capturing for a few seconds.
    - [ ] Assert that "received shutdown" log messages exist for all key components.
    - [ ] Assert that no "is alive" heartbeat messages are present in the logs after the shutdown signal was sent.


---
### Epic 28: Robust Shutdown for DNS Resolution
**User Story:** As an operator, I want the application to shut down promptly (within a few seconds) when I press Ctrl-C, even when it is under heavy load from a high-volume pattern match, so that I can restart or redeploy the service without long delays.

- [x] **#87 - Make DNS Resolution Shutdown-Aware**
  - **Context:** The current DNS resolution logic can block a worker task for an extended period, waiting for a DNS query to time out. This prevents the worker from observing the shutdown signal in a timely manner. This task will make the DNS resolution process itself cancellable.
  - **Dependencies:** #76.1
  - **Subtasks:**
    - [x] In `src/dns.rs`, modify the `DnsResolutionManager::resolve_with_retry` function to accept the shutdown signal receiver.
    - [x] Inside the function, wrap the `hickory-resolver`'s `lookup_ip` future in a `tokio::select!` block.
    - [x] The `select!` will race the DNS lookup against the shutdown signal.
    - [x] If the shutdown signal is received first, the function will immediately return a distinct error (e.g., `DnsError::Shutdown`) to the calling worker.
    - [x] The worker will interpret this error as a signal to terminate immediately.

- [x] **#88 - Propagate Shutdown Signal to DNS Manager**
  - **Context:** The `DnsResolutionManager` needs access to the application's shutdown signal to pass it to the resolution logic.
  - **Dependencies:** #87
  - **Subtasks:**
    - [x] In `main.rs`, when creating the `DnsResolutionManager`, pass a clone of the shutdown signal receiver to its constructor.
    - [x] Update the `DnsResolutionManager::new` function signature to accept the receiver.
    - [x] In the main worker loop, pass the shutdown signal to `resolve_with_retry`.
