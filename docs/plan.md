### Epic 27: Anti-Regression Testing for Graceful Shutdown
**User Story:** As a developer, I want a robust suite of automated tests that can reliably detect shutdown hangs and deadlocks, so that this entire class of bug can be prevented from recurring in the future.


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
### Epic 32: Simplify Configuration to be File-Only
**User Story:** As a developer, I want a simple, robust, and predictable configuration system, so that I can avoid complex debugging sessions caused by unpredictable configuration layering between defaults, files, and command-line arguments.
**Technical Goal:** Refactor the configuration system to be exclusively file-driven. The application will be configured via a single TOML file, with `clap` only being used to parse the path to that file and a test-only flag. This eliminates the complex and error-prone interaction between `figment` and `clap`.

- [x] **#97 - Simplify `Config` Struct and Loading Logic**
  - **Context:** The core of the refactor. The `Config` struct was stripped of all `clap` and `figment` attributes, becoming a pure data container. The loading logic was simplified to a two-step process: start with `Config::default()` and merge a single TOML file over it.
  - **Subtasks:**
    - [x] Removed all `clap::Parser` and `figment` attributes from `src/config.rs`.
    - [x] Implemented `Default` for `Config` and its sub-structs to provide the baseline configuration.
    - [x] Created a minimal `Cli` struct in `src/main.rs` to parse only `--config-file` and `--test-mode`.
    - [x] Updated `Config::load()` to be driven by the `Cli` input.

- [x] **#98 - Remove Redundant Code and Features**
  - **Context:** With the move to a file-only strategy, all code related to command-line overrides for individual settings became obsolete.
  - **Subtasks:**
    - [x] Deleted the `src/cli.rs` file and its `mod` declaration.
    - [x] Removed the `env` feature from the `clap` dependency in `Cargo.toml`.
    - [x] Stripped out all logic for layering environment variables and individual CLI flags.

- [x] **#99 - Update Integration Tests to be File-Driven**
  - **Context:** The existing integration tests relied heavily on passing command-line arguments. They were completely rewritten to create temporary TOML configuration files for each test case.
  - **Subtasks:**
    - [x] Rewrote all tests in `tests/integrations/config.rs` to use `tempfile` and `figment` to create and load test-specific configuration files.
    - [x] Updated tests in other modules, like `tests/enrichment/startup.rs`, that were also using the old CLI argument system.

- [x] **#100 - Cleanup and Documentation**
  - **Context:** After the major architectural change, a thorough cleanup and documentation pass was required to bring the project into a consistent state.
  - **Subtasks:**
    - [x] Reviewed all changed files for dead code and simplification opportunities.
    - [x] Updated `README.md` to describe the new file-only configuration and the two remaining command-line arguments.
    - [x] Verified and updated `certwatch-example.toml` to be a complete and accurate template for users.
    - [x] Ran the full test suite (`cargo test --all-features`) to ensure no regressions were introduced.

---

### Epic #40: Refactor `dns` Module Architecture

- **User Story:** As a developer, I want the `dns` module to follow the same clean, organized structure as the `enrichment` module, so that the codebase is more consistent, easier to navigate, and simpler to maintain.
- **Acceptance Criteria:**
  - The `src/dns` directory is created and populated with `mod.rs`, `health.rs`, `resolver.rs`, and `fake.rs`.
  - The `DnsResolver` trait is defined in `src/dns/mod.rs`.
  - The `startup_check` function is located in `src/dns/health.rs`.
  - Production resolvers (`HickoryDnsResolver`, `NoOpDnsResolver`) are in `src/dns/resolver.rs`.
  - The test-only `FakeDnsResolver` is in `src/dns/fake.rs` and conditionally compiled.
  - The old `src/dns.rs` and `src/dns/health.rs` files are deleted.
  - The application compiles and all tests pass after the refactoring.
- **Tasks:**
  - [ ] **#126 - Create new `dns` module structure**
    - [ ] Create the directory `src/dns`.
    - [ ] Create the files `src/dns/mod.rs`, `src/dns/health.rs`, `src/dns/resolver.rs`, and `src/dns/fake.rs`.
  - [ ] **#127 - Migrate `DnsResolver` trait and implementations**
    - [ ] Move `DnsResolver` trait to `src/dns/mod.rs`.
    - [ ] Move `HickoryDnsResolver` and `NoOpDnsResolver` to `src/dns/resolver.rs`.
    - [ ] Move `FakeDnsResolver` to `src/dns/fake.rs`.
  - [ ] **#128 - Migrate `startup_check` function**
    - [ ] Move the `startup_check` function to `src/dns/health.rs`.
  - [ ] **#129 - Update codebase and remove old files**
    - [ ] Update all module paths across the codebase to point to the new locations.
    - [ ] Delete `src/dns.rs` and the old `src/dns/health.rs`.
    - [ ] Run `cargo check` and `cargo test --all-features` to ensure everything works correctly.


---

### Epic #41: Harden Test Suite and Improve Error Propagation

- **User Story:** As a developer, I want the test suite to be more robust and reliable, and I want to ensure that errors are correctly propagated throughout the application so that we can catch regressions more effectively.
- **Acceptance Criteria:**
  - The `dns_failure.rs` test is refactored to be a focused unit test that is no longer flaky.
  - The `TestMetrics` helper is improved to support resetting counters between test scenarios.
  - Errors from the `build_alert` function are correctly propagated and handled in `app::run`.
  - The `enrichment_failure.rs` test correctly verifies that enrichment failures are recorded.
- **Tasks:**
  - [ ] **#133 - Fix flaky `test_enrichment_failure_increments_failure_metric` test.**


---
---

### **Phase 9: Formalize the Extensible Enrichment Framework**
*   **Goal:** Refactor the internal architecture to be a generic, multi-level pipeline, preparing for future high-cost enrichment stages.
*   **Status:** Not Started
*   **Tasks:**
    *   [ ] **#188 - EnrichmentLevel Enum:** Create a formal `EnrichmentLevel` enum (e.g., `Level0`, `Level1`, `Level2`).
    *   [ ] **#189 - Condition Trait:** Define a trait or method for rule conditions to report their required `EnrichmentLevel`.
    *   [ ] **#190 - Generic Pipeline:** Refactor the hardcoded two-stage pipeline into a generic loop that progresses through enrichment levels, filtering at each step. This is primarily an internal code quality improvement.

---

### **Phase 10: Add a New, High-Cost Enrichment Stage (Proof of Concept)**
*   **Goal:** Prove the framework's extensibility by adding a new, expensive check.
*   **Status:** Not Started
*   **Tasks:**
    *   [ ] **#191 - Define New Condition:** Add a hypothetical `http_body_matches` condition and assign it a new, higher `EnrichmentLevel`.
    *   [ ] **#192 - Implement Enrichment Provider:** Create a new enrichment provider that can perform the HTTP request.
    *   [ ] **#193 - Integration Test:** Create an integration test demonstrating that the new provider is only called for alerts that have successfully passed all lower-level filter stages.

---

### **Epic #47: Modernize Metrics System with Prometheus Exporter**

*   **Goal:** Replace the current in-memory, log-based metrics system with a production-grade Prometheus exporter. This will provide real-time visibility into application performance and align with industry-standard observability practices.
*   **Status:** In Progress
*   **Tasks:**
    *   [x] **#194 - Dependencies:** Add `metrics-exporter-prometheus` and `axum` to `Cargo.toml`.
    *   [x] **#195 - Configuration:** Add a `metrics` section to `config.rs` to enable/disable the exporter and configure its listen address.
    *   [ ] **#196 - Prometheus Recorder:** Create `src/internal_metrics/prometheus_recorder.rs` to initialize and manage the Prometheus exporter.
    *   [ ] **#197 - Exporter Server:** Implement a function to run a lightweight `axum` server in a separate Tokio task, serving the rendered metrics at the `/metrics` endpoint.
    *   [ ] **#198 - Application Integration:** In `main.rs`, conditionally initialize the `PrometheusRecorder` and start the server based on the new configuration.
    *   [ ] **#199 - Preserve Test Integrity:** Update the `TestAppBuilder` to ensure all existing tests continue to use the `TestMetricsRecorder`, avoiding any disruption to our test suite.
    *   [ ] **#200 - Documentation:** Update project documentation to explain the new metrics system.

---

### Epic #48: Implement Rule Hot-Reloading
*   **Goal:** Re-introduce the hot-reloading feature, making it compatible with the new YAML-based rules engine. This allows for dynamic updates to the matching logic without requiring a service restart, restoring a key feature from the legacy system.
*   **Architecture:** A new `RuleWatcher` component will be created. It will use the `notify` crate to monitor the rule files for changes. When a change is detected, it will attempt to load the new rules. If successful, it will broadcast the new `Arc<RuleMatcher>` to all worker tasks using a `tokio::sync::watch` channel. This ensures a non-blocking, clean distribution of the updated configuration.
    ```mermaid
    graph TD
        subgraph Rule Watcher Task
            A[File System Event] --> B{Change Detected};
            B --> C[Load New Rules];
            C -- Success --> D[Send Arc<RuleMatcher>];
            C -- Failure --> E[Log Error & Keep Old Rules];
        end
        subgraph Main Application
            F[app::run] --> G{Create watch channel};
            G --> H[Spawn Rule Watcher];
            G --> I[Spawn Workers];
        end
        subgraph Worker Task
            J[Start Loop] --> K{Select};
            K -- Domain Received --> L[Process with Current Rules];
            K -- Rule Update Received --> M[Update Local Arc<RuleMatcher>];
            L --> J;
            M --> J;
        end
        D -- watch::Sender --> G;
    ```
*   **Status:** Not Started
*   **Tasks:**
    *   [ ] **#213 - Dependency:** Add the `notify` and `notify-debouncer-full` crates to `Cargo.toml`.
    *   [ ] **#214 - Config:** Add a `hot_reload: bool` flag to the `[rules]` section in `certwatch.toml` and the `Config` struct to enable/disable this feature.
    *   [ ] **#215 - Component:** Create `src/rule_watcher.rs` and implement the `RuleWatcher` which will manage file watching and rule reloading.
    *   [ ] **#216 - Integration:** In `src/app.rs`, create the `watch` channel and spawn the `RuleWatcher` task if hot-reloading is enabled. Pass the `watch::Receiver` to the workers.
    *   [ ] **#217 - Worker Update:** Modify the worker loop in `src/app.rs` to listen for rule updates on the `watch::Receiver` and swap its local `RuleMatcher` when a new version is available.
    *   [ ] **#218 - Testing:** Create `tests/rule_hot_reload.rs` to verify the end-to-end hot-reloading functionality, including success and failure cases (e.g., invalid rule syntax).
    *   [ ] **#219 - Docs:** Update `README.md` and `certwatch-example.toml` to remove the obsolete `[matching]` section and accurately describe the new hot-reloading feature for YAML rules.


---

### Epic #49: Testing Workflow Overhaul
*   **Goal:** Standardize the project's testing workflow by replacing the ad-hoc `cargo test` command with `cargo nextest` and introducing a `justfile` as the canonical task runner. This will improve consistency, discoverability, and maintainability of project-level commands.
*   **Status:** Completed
*   **Tasks:**
    *   [x] **#220 - Create `justfile`:** A new file named `justfile` was created in the project root to serve as the central place for defining and documenting project-specific commands.
    *   [x] **#221 - Update `.clinerules`:** The `Task Completion Protocol` section in `.clinerules` has been updated to replace the `cargo test` command with the new `just test` command.
    *   [x] **#222 - Verify Workflow:** The new `just test` command has been run to ensure it works as expected.


---

### Epic #50: Ensure Clean JSON Output for Pipelining
*   **User Story:** As a command-line user, I want `certwatch` to send only JSON data to standard output (`stdout`), so that I can reliably pipe the output to other tools like `jq` for filtering and analysis without encountering parsing errors.
*   **Status:** Completed
*   **Tasks:**
    *   [x] **#223 - Reconfigure Tracing Subscriber:** Modify the `tracing_subscriber` in `main.rs` to write all logs to `stderr` instead of `stdout`.
    *   [x] **#224 - Audit `println!` Usage:** Perform a codebase-wide audit to find and replace any `println!` macros used for logging with appropriate `tracing` macros.
    *   [x] **#225 - Create Output Verification Test:** Implement a new integration test that runs the application with JSON output, captures both `stdout` and `stderr`, and asserts that only valid JSON is written to `stdout` and logs are correctly written to `stderr`.

---

### Epic #51: Enhance Slack Alerts with Deduplicated Domain Counts
*   **User Story:** As a security analyst receiving CertWatch alerts, I want to see a count of suppressed subdomains and IPs in a compact format, so that I can quickly gauge the magnitude of an alert without unnecessary verbosity.
*   **Status:** Not Started
*   **Tasks:**
    *   [ ] **#226 - Update Slack Formatting Logic:** Modify the Slack notification formatting to include a `(+x more)` count for both deduplicated subdomains and aggregated IP addresses.
    *   [ ] **#227 - Refactor IP Aggregation Text:** Change the existing `(+x others)` for IPs to `(+x more)` for consistency.
    *   [ ] **#228 - Add Conditional Display Logic:** Ensure the `(+x more)` text only appears when the count of additional items is greater than zero.
    *   [ ] **#229 - Update Integration Tests:** Modify existing Slack notification tests to assert the new format, including cases with and without the `(+x more)` text.
