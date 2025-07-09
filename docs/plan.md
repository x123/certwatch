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
### Epic 32: Unify Configuration and CLI Parsing
**User Story:** As a power user or system administrator, I want to configure every application setting using command-line arguments, so that I can run the application in automated or ephemeral environments (like Docker or CI/CD) without needing to manage a separate configuration file.
**Technical Goal:** Refactor the configuration and CLI parsing logic to use a single source of truth. The `Config` struct will be decorated with `clap::Parser` attributes, eliminating the separate `Cli` struct and the manual mapping between them.

- [ ] **#97 - Refactor `Config` to be the `clap` Parser**
  - **Context:** The core of the refactor. The `Config` struct and its sub-structs will be decorated with `clap` attributes, making them the single source of truth for all configuration, including CLI flags.
  - **Dependencies:** #10
  - **Subtasks:**
    - [ ] In `Cargo.toml`, ensure `clap` with the `derive` feature is a dependency of the `certwatch` library.
    - [ ] In `src/config.rs`, add `#[derive(Parser)]` to `Config` and `#[derive(Args)]` to all sub-structs.
    - [ ] Add `#[command(flatten)]` to all fields that are sub-structs.
    - [ ] Add `#[arg(long = "...")]` to every individual configuration field, defining its command-line flag.
    - [ ] Ensure all fields that can be layered are `Option<T>` to work correctly with `figment`'s merging.
    - [ ] Add a dedicated, `#[serde(skip)]`-ed field to `Config` for the `--config` file path argument.

- [ ] **#98 - Update `main` to Use the New Unified Config Strategy**
  - **Context:** The application's entry point must be updated to use the new parsing and layering logic.
  - **Dependencies:** #97
  - **Subtasks:**
    - [ ] In `main.rs`, replace the old `Cli::parse()` and `Config::load()` logic.
    - [ ] First, call `Config::parse()` to get a partial `Config` object containing only the values provided on the command line.
    - [ ] Then, use `figment` to layer the configuration sources correctly: `Defaults -> TOML File -> Environment -> CLI Arguments`.
    - [ ] Ensure the final, merged `Config` is extracted successfully.

- [ ] **#99 - Remove Redundant CLI Code**
  - **Context:** With the `Config` struct now handling CLI parsing, the old `Cli` struct and its `figment::Provider` implementation are obsolete.
  - **Dependencies:** #98
  - **Subtasks:**
    - [ ] Delete the `src/cli.rs` file.
    - [ ] Remove the `mod cli;` declaration from `src/lib.rs`.
    - [ ] Remove any `use certwatch::cli::Cli;` statements.

- [ ] **#100 - Validate the Refactoring**
  - **Context:** After such a significant refactoring, it's critical to verify that all functionality remains intact and the new CLI works as expected.
  - **Dependencies:** #99
  - **Subtasks:**
    - [ ] Run `cargo test --all-features` to ensure no regressions.
    - [ ] Run `certwatch --help` and verify that the generated help message is comprehensive and correctly documents all available flags.

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
