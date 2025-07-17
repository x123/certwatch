### Epic #55: Granular Performance Instrumentation for Latency Analysis

*   **User Story:** As a developer, I need detailed performance metrics for each stage of the domain processing pipeline, so that I can accurately identify and diagnose latency bottlenecks that are not visible with the current high-level metrics.

*   **Architecture:** The current `processing_duration_seconds` histogram measures the entire worker loop, but the extreme latency observed suggests a bottleneck within a specific, unmeasured part of that loop. The plan is to add several new, more granular histograms to the rules worker loop in `src/app.rs` one by one, testing at each stage to ensure stability. This incremental approach minimizes risk and allows for careful validation of each change.

*   **Status:** Not Started

*   **Phase 1: Instrument the Worker Loop Iteration**
    *   **Goal:** Establish a baseline by measuring the entire worker loop duration.
    *   **Tasks:**
        *   [x] **#242 - Define `worker_loop_iteration_duration_seconds`:** In `src/internal_metrics/mod.rs`, add the `describe_histogram!` definition.
        *   [x] **#243 - Implement `worker_loop_iteration_duration_seconds`:** In `src/app.rs`, add logic to record the duration of a single iteration of the rules worker loop.
        *   [x] **#244 - Verify Implementation:** Run the test suite and manually inspect the `/metrics` endpoint to ensure the new metric is present and recording data.

*   **Phase 2: Instrument the Alert Building Stage**
    *   **Goal:** Isolate the performance of the `build_alert()` function, a primary suspect for latency.
    *   **Tasks:**
        *   [x] **#245 - Define `alert_build_duration_seconds`:** In `src/internal_metrics/mod.rs`, add the `describe_histogram!` definition.
        *   [x] **#246 - Implement `alert_build_duration_seconds`:** In `src/app.rs`, wrap the `build_alert().await` call to record its duration.
        *   [x] **#247 - Verify Implementation:** Run tests and inspect the `/metrics` endpoint to confirm the new metric is working correctly alongside the first one.

*   **Phase 3: Instrument the Rule Matching Stage**
    *   **Goal:** Measure the overhead of the rule matching logic itself, separate from the underlying regex performance.
    *   **Tasks:**
        *   [x] **#248 - Define `rule_matching_duration_seconds`:** In `src/internal_metrics/mod.rs`, add the `describe_histogram!` definition.
        *   [x] **#249 - Implement `rule_matching_duration_seconds`:** In `src/app.rs`, add logic to record the duration of the `rule_matcher.matches()` calls.
        *   [x] **#250 - Verify Implementation:** Run tests and inspect the `/metrics` endpoint.

*   **Phase 4: Instrument the Alert Sending Stage**
    *   **Goal:** Measure the time it takes to send an alert to the output channel to detect potential backpressure.
    *   **Tasks:**
        *   [x] **#251 - Define `alert_send_duration_seconds`:** In `src/internal_metrics/mod.rs`, add the `describe_histogram!` definition.
        *   [x] **#252 - Implement `alert_send_duration_seconds`:** In `src/app.rs`, wrap the `alerts_tx.send().await` call to record its duration.
        *   [x] **#253 - Verify Implementation:** Run tests and inspect the `/metrics` endpoint.

*   **Phase 5: Analysis and Cleanup**
    *   **Goal:** Analyze the complete set of new metrics to identify the bottleneck and clean up temporary instrumentation.
    *   **Tasks:**
        *   [ ] **#254 - Analyze All New Metrics:** Capture and analyze the full suite of new metrics from a running instance to definitively identify the latency source.

---

### Epic #54: Code Cleanup and Warning Resolution

*   **User Story:** As a developer, I want a codebase that compiles without any warnings, so that I can maintain high code quality, improve readability, and ensure that potential issues are not being masked by noise.

*   **Architecture:** The approach will be to methodically address each warning category identified by the Rust compiler. This involves removing unused imports, variables, and functions from the test helper modules and integration tests. Each change will be small, targeted, and validated by running the test suite to ensure no regressions are introduced.

*   **Status:** Completed

*   **Tasks:**
    *   [x] **#238 - Clean up `tests/helpers/app.rs`:**
        *   **Subtask:** Remove the unused `Mutex` import.
        *   **Subtask:** Prefix the unused `channel` variable in `with_slack` with an underscore (`_channel`) to silence the warning.

    *   [x] **#239 - Clean up `tests/helpers/mock_slack.rs`:**
        *   **Subtask:** Remove the unused `new` and `get_sent_batches` functions, as they are no longer required by any tests.

    *   [x] **#240 - Clean up integration tests:**
        *   **Subtask:** Remove the unused `create_rule_file` import from `tests/integrations/work_distribution.rs`.
        *   **Subtask:** Remove the unused `FakeEnrichmentProvider` and `Arc` imports from `tests/integrations/not_rules.rs`.

    *   [x] **#241 - Final Verification:**
        *   **Subtask:** Run the full test suite (`just test`) one final time to ensure the project compiles cleanly with zero warnings and all tests pass.

---

### Epic #53: Decouple Slack Client from Tokio Runtime

*   **User Story:** As a developer, I want to ensure that slow or unresponsive Slack API calls do not interfere with the core application's network stability, so that the CertStream websocket connection remains stable and responsive.

*   **Problem:** The current `SlackClient` implementation uses a standard `reqwest::Client` and `await`s the HTTP request directly within a Tokio task. This is a blocking I/O operation that can tie up a worker thread in the Tokio runtime. If the Slack API is slow, this prevents other critical asynchronous tasks, such as the CertStream websocket client, from being processed. This leads to missed keep-alive messages (pings/pongs) and causes the websocket connection to be dropped, either through a timeout or a graceful close from the server.

*   **Architecture:** The solution is to move the blocking HTTP request to a dedicated thread pool where it won't interfere with the main Tokio runtime. This will be achieved using `tokio::task::spawn_blocking`. A new, private, blocking function will be created to handle the synchronous `reqwest` call. The public `send_batch` function will become a lightweight async wrapper that uses `spawn_blocking` to safely execute the synchronous code without stalling the main event loop.

    ```mermaid
    graph TD
        subgraph NotificationManager Task (Async)
            A[NotificationManager::send_batch] --> B{tokio::task::spawn_blocking};
        end

        subgraph Blocking Thread Pool
            C[SlackClient::send_request] --> D[reqwest::blocking::Client];
            D --> E[Sends HTTP Request to Slack];
        end

        B -- Schedules --> C;
        E -- Returns Result --> A;
    ```

*   **Status:** Not Started

*   **Tasks:**
    *   [ ] **#235 - Create Blocking Request Function:**
        *   **Subtask:** In `src/notification/slack.rs`, create a new private function `send_request` that accepts a `reqwest::blocking::Client` and the JSON payload.
        *   **Subtask:** This function will contain the existing synchronous logic for sending the HTTP request and handling the response.

    *   [ ] **#236 - Refactor `send_batch` to be Non-Blocking:**
        *   **Subtask:** Modify the existing `send_batch` function to be an `async` function.
        *   **Subtask:** Inside `send_batch`, use `tokio::task::spawn_blocking` to call the new `send_request` function.
        *   **Subtask:** A new `reqwest::blocking::Client` will be created inside the `spawn_blocking` closure, as it is not `Send`.

    *   [ ] **#237 - Verify Test Coverage:**
        *   **Subtask:** Review the existing tests in `src/notification/slack.rs` to ensure they still provide adequate coverage for both success and failure cases after the refactoring.
        *   **Subtask:** Run the full integration test suite (`just test`) to confirm that the changes have not introduced any regressions.

---

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

---

### Epic #52: Formalize the Extensible Enrichment Framework

*   **User Story:** As a developer, I want to refactor the hardcoded two-stage filtering pipeline into a generic, multi-level pipeline, so that new, high-cost enrichment stages can be added in the future with minimal code changes and without impacting performance unnecessarily.

*   **Vision & Justification:**
    The current implementation in `process_domain` is a rigid, two-stage process. It first checks rules that require no enrichment (`EnrichmentLevel::None`), and if they match, it *always* proceeds to perform DNS and ASN enrichment before checking the next stage of rules (`EnrichmentLevel::Standard`).

    This architecture has two key weaknesses:
    1.  **Inextensible:** Adding a new, more expensive enrichment stage (e.g., a hypothetical HTTP lookup for `EnrichmentLevel::HighCost`) would require invasive changes to the core `process_domain` function, adding more `if/else` blocks and further coupling the application logic.
    2.  **Inefficient:** It lacks the granularity to support multiple enrichment levels beyond the current one. It cannot, for example, run a cheap DNS check for one set of rules and a more expensive ASN lookup only for a smaller subset.

    This refactoring will replace the hardcoded logic with a generic, iterative pipeline. This new design will be driven by an ordered `EnrichmentLevel` enum, ensuring that we only perform the minimum work necessary at each stage. This makes the system highly extensible and more efficient, paving the way for future features without requiring complex rewrites.

*   **Architecture:**
    The core of the new design is a loop within `process_domain` that iterates through the `EnrichmentLevel` enum.
    1.  A new `EnrichmentManager` will be created to hold all available `EnrichmentProvider` implementations, mapped to the level they provide.
    2.  The `process_domain` loop will start at `EnrichmentLevel::None`.
    3.  In each iteration, it will ask the `RuleMatcher` for rules matching the *current* level.
    4.  If no rules match, the process stops for that domain.
    5.  If rules *do* match, the loop advances to the *next* enrichment level. It asks the `EnrichmentManager` for the appropriate provider, executes it, and merges the new data into the `Alert`.
    6.  The loop continues until all enrichment levels have been processed or the domain has been filtered out.

    ```mermaid
    graph TD
        A[Start: Domain Received] --> B{Loop: Current Level = None};
        B --> C{"Match Rules at Current Level?"};
        C -- No --> X[Discard Domain];
        C -- Yes --> D{Get Next Enrichment Level};
        D -- No More Levels --> J[Final Alert Sent];
        D -- Next Level Exists --> E[Get Provider from EnrichmentManager];
        E --> F[Execute Enrichment];
        F --> G[Merge Data into Alert];
        G --> B;
    ```

*   **Status:** Not Started

*   **Tasks:**
    *   [ ] **#230 - Foundational Type & Trait Enhancements**
        *   **Subtask:** Add the `strum` and `strum_macros` crates to `Cargo.toml` to make the `EnrichmentLevel` enum easily iterable.
        *   **Subtask:** Derive `EnumIter` for `EnrichmentLevel` in `src/rules.rs` and ensure its variants are ordered correctly (`None`, `Standard`, etc.).
        *   **Subtask:** Create a new `EnrichmentProvider` trait method, `provides_level() -> EnrichmentLevel`, to allow providers to declare what level of data they supply.

    *   [ ] **#231 - Create the `EnrichmentManager`**
        *   **Subtask:** Create a new file `src/enrichment/manager.rs`.
        *   **Subtask:** Implement the `EnrichmentManager` struct, which will hold a `HashMap<EnrichmentLevel, Arc<dyn EnrichmentProvider>>`.
        *   **Subtask:** Implement a `new()` or `build()` function for the manager that takes the application `Config` and instantiates all required providers (e.g., `TsvAsnLookup`).
        *   **Subtask:** Implement a `get_provider_for(&self, level: EnrichmentLevel) -> Option<...>` method.

    *   [ ] **#232 - Implement the Generic Pipeline in `app.rs`**
        *   **Subtask:** In `AppBuilder`, replace the single `enrichment_provider_override` with the new `EnrichmentManager`.
        *   **Subtask:** In `process_domain`, remove the existing two-stage logic.
        *   **Subtask:** Implement the new iterative loop as described in the architecture diagram.
        *   **Subtask:** The loop should maintain a set of matched rule names and only proceed to the next enrichment level if there are matching rules from the current level.

    *   [ ] **#233 - Write Focused Unit Tests**
        *   **Subtask:** In `src/app.rs`'s test module, create a new test to verify that a domain that doesn't match `Level::None` rules is correctly discarded without any enrichment.
        *   **Subtask:** Create a test to verify that a domain matching `Level::None` rules proceeds to `Level::Standard` enrichment.
        *   **Subtask:** Create a test to verify that a domain matching both `Level::None` and `Level::Standard` rules results in a final alert with all rule names.

    *   [ ] **#234 - Update Integration Tests**
        *   **Subtask:** Update the `TestAppBuilder` in `tests/helpers/app.rs` to construct and use the new `EnrichmentManager`.
        *   **Subtask:** Review and update existing integration tests (e.g., `tests/integrations/not_rules.rs`) to ensure they are compatible with the new pipeline logic. The `with_failing_enrichment` helper may need to be adapted.
