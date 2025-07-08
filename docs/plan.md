---
### Epic 37: Unplanned Maintenance &amp; Bug Fixes
**User Story:** As a developer, I want to quickly address critical bugs and regressions to ensure the application remains stable and reliable for users.

- [x] **#122 - Fix Startup Crash Due to Missing ASN Database**
  - **Context:** A regression was introduced that caused the application to panic and exit on startup if the optional ASN TSV database file was not found at the path specified in the configuration.
  - **Subtasks:**
    - [x] Used `git bisect` to identify the faulty commit (`e0866a59`).
    - [x] Analyzed the commit to understand the root cause of the regression.
    - [x] Modified `src/app.rs` to handle the missing file gracefully by logging a warning and disabling the enrichment feature, rather than crashing.
    - [x] Verified the fix by running the application and confirming it no longer panics.

---
### Epic 35: Observability and Error Handling
**User Story:** As a developer, I want to have clear, top-level logs and metrics for domain processing, so that I can easily monitor the application's health and diagnose failures without digging through deep call stacks.

- [x] **#108 - Improve Error Handling in Core Processing Loop**
  - **Context:** The current worker loop in `src/app.rs` ignores the result of the main processing logic, making it difficult to track failures. This task will introduce explicit error handling, logging, and metrics.
  - **Subtasks:**
    - [x] Extract the domain processing logic into a separate, fallible `process_domain` function in `src/app.rs`.
    - [x] Update the worker loop to call this function and handle the `Result`.
    - [x] On failure, log a `warn!` message including the domain and error.
    - [x] On failure, increment a `cert_processing_failures` metric.
    - [x] On success, increment a `cert_processing_successes` metric.
    - [x] Ensure the graceful shutdown mechanism is preserved by correctly using `tokio::select!`.

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
### Epic 30: Architectural Refactoring
**User Story:** As a developer, I want the application's startup and component wiring logic to be clean, testable, and maintainable, so that I can easily add new features or modify existing ones without breaking the application.
**Technical Goal:** Refactor the monolithic `main` function into a structured `App` and `AppBuilder` pattern. This will decouple component construction from the application's entry point, improving modularity and testability.

- [ ] **#91 - Implement AppBuilder Pattern**
  - **Context:** Create a new `AppBuilder` struct responsible for the step-by-step construction of the application's services and components.
  - **Dependencies:** #11
  - **Subtasks:**
    - [ ] Create a new `src/app.rs` module.
    - [ ] Define an `App` struct to hold the application's state (services, task handles, shutdown channel).
    - [ ] Define an `AppBuilder` struct with methods to configure and add each service (e.g., `with_dns_resolver`, `with_pattern_matcher`).
    - [ ] The `AppBuilder::build()` method will consume the builder and return a fully configured `App` instance.

- [ ] **#92 - Refactor `main` to use AppBuilder**
  - **Context:** Simplify the `main` function to delegate all construction and wiring logic to the `AppBuilder`.
  - **Dependencies:** #91
  - **Subtasks:**
    - [ ] Move all service instantiation logic from `main.rs` into the appropriate methods of `AppBuilder`.
    - [ ] The `main` function will be reduced to: loading config, creating the builder, calling `build()`, and then `app.run()`.

- [ ] **#93 - Implement `App::run`**
  - **Context:** Create the main execution logic within the `App` struct.
  - **Dependencies:** #91
  - **Subtasks:**
    - [ ] Create an `async fn run(&mut self)` method on the `App` struct.
    - [ ] This method will be responsible for spawning all the application's tasks (CertStream client, worker pool, output manager, etc.).
    - [ ] It will also contain the logic for waiting for the shutdown signal and gracefully terminating all spawned tasks.

---
### Epic 31: Code Quality and Cleanup
**User Story:** As a developer, I want the codebase to be clean, consistent, and well-documented, so that it is easy to understand, maintain, and contribute to.
**Technical Goal:** Address various pieces of technical debt, inconsistent patterns, and documentation gaps identified during the pre-release code review.

- [ ] **#94 - Improve Service Encapsulation**
  - **Context:** Services should be responsible for their own configuration and input validation, rather than having this logic in `main.rs`.
  - **Dependencies:** #10
  - **Subtasks:**
    - [ ] Modify service constructors (e.g., `DnsResolutionManager::new`) to accept their specific config structs (e.g., `DnsConfig`) instead of a long list of primitive values.
    - [ ] Move the `asn_tsv_path` existence check from `main.rs` into the `TsvAsnLookup::new()` constructor.
    - [ ] Move the `build_alert` helper function to be an associated function `Alert::new()`.

- [ ] **#95 - Refactor Duplicated Logic**
  - **Context:** Repetitive code blocks should be extracted into shared helper functions to reduce duplication and improve maintainability.
  - **Dependencies:** #24
  - **Subtasks:**
    - [ ] The heartbeat spawning logic is duplicated for the `QueueMonitor` and `OutputManager`. Extract this into a `utils::spawn_heartbeat_task` helper function.

- [ ] **#96 - Enhance Documentation and Code Clarity**
  - **Context:** Improve inline documentation to clarify complex or non-obvious parts of the code.
  - **Dependencies:** All epics
  - **Subtasks:**
    - [ ] Add `// WHY:` comments to all `tokio::select!` blocks that use the `biased;` keyword, explaining the reasoning for prioritizing one branch over others.
    - [ ] Review and improve the documentation for the `core` traits to more clearly define the contract for each method, especially error conditions and expected return values.


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
### Epic 34: Modular Rule Engine for Flexible Pattern Matching

**User Story:**
As a security analyst or operator, I want to define flexible "rules" that group selectors (domain regex, IP/CIDR, ASN, country, org, etc.) and associate them with custom actions, so that I can precisely control which events trigger which outputs and reduce noise.

- [ ] **#104 - Build Rule Engine Core**
  - **Context:** Implement rule evaluation logic with logical composition, support multiple rules and overlapping matches, and integrate with the enrichment data pipeline.
  - **Dependencies:** #103
  - **Subtasks:**
    - [ ] Implement rule evaluation logic with logical composition.
    - [ ] Support multiple rules and overlapping matches.
    - [ ] Integrate with enrichment data pipeline.

- [ ] **#105 - Hot-Reload and Validation**
  - **Context:** Implement hot-reload for rule configuration, validate rules at load time with clear error reporting, and handle edge cases (invalid selectors, missing enrichment, performance with large rule sets).
  - **Dependencies:** #104
  - **Subtasks:**
    - [ ] Implement hot-reload for rule configuration.
    - [ ] Validate rules at load time, with clear error reporting.
    - [ ] Handle edge cases: invalid selectors, missing enrichment, performance with large rule sets.

- [ ] **#106 - Integrate Rule Engine into Main Pipeline**
  - **Context:** Replace or augment existing pattern matching with the rule engine, ensure compatibility with output and alerting systems, and add integration tests for end-to-end rule evaluation and action dispatch.
  - **Dependencies:** #105
  - **Subtasks:**
    - [ ] Replace or augment existing pattern matching with rule engine.
    - [ ] Ensure compatibility with output and alerting systems.
    - [ ] Add integration tests for end-to-end rule evaluation and action dispatch.

- [ ] **#107 - Documentation and Examples**
  - **Context:** Document rule configuration and usage in `docs/specs.md` and `README.md`, and provide example rule sets for common use cases.
  - **Dependencies:** #106
  - **Subtasks:**
    - [ ] Document rule configuration and usage in `docs/specs.md` and `README.md`.
    - [ ] Provide example rule sets for common use cases.


---
### Epic 36: Enhance Test Suite with Failure Injection
**User Story:** As a developer, I want to test the application's resilience against common failure modes, so that I can be confident it will behave predictably and not crash when external dependencies fail.

- [x] **#109 - Phase 1: Implement Mockable Alert Sink**
  - **Context:** The application should be resilient to failures when sending alerts (e.g., Slack API is down). We need to test this behavior.
  - **Subtasks:**
    - [x] Create a `FailableMockOutput` in `tests/helpers/` that implements the `Output` trait.
    - [x] The mock is configurable to return an error on `send`.
    - [x] It records alerts it receives for later inspection.
    - [x] Modify `certwatch::app::run` to accept an optional, pre-built `Vec<Arc<dyn Output>>`.
    - [x] Modify `TestAppBuilder` to allow injecting the mock outputs.
    - [x] Write an integration test that injects a failing `FailableMockOutput`, triggers an alert, and verifies the application logs a warning and continues running.

- [x] **#110 - Phase 2: Implement Fake Enrichment Provider**
  - **Context:** The application should handle failures from the enrichment service gracefully.
  - **Subtasks:**
    - [x] Create a `FakeEnrichmentProvider` in `tests/helpers/` that implements the `EnrichmentProvider` trait.
    - [x] The fake is configurable to return an error.
    - [x] Modify `certwatch::app::run` to accept an optional, pre-built `Arc<dyn EnrichmentProvider>`.
    - [x] Modify `TestAppBuilder` to allow injecting the `FakeEnrichmentProvider`.
    - [x] Write an integration test that injects a failing `FakeEnrichmentProvider`, processes a domain, and verifies the `cert_processing_failures` metric is incremented.
    - [x] Refactor `build_alert` to propagate errors instead of logging them, enabling the failure to be caught by the worker loop.

- [x] **#111 - Phase 3: Implement Failing DNS Resolver**
  - **Context:** The application must be robust against DNS resolution failures, which are common network issues. We will test the application's reaction to these failures without mocking a full DNS server.
  - **Subtasks:**
    - [x] Create a `MockDnsResolver` in `tests/helpers/` that implements the `DnsResolver` trait.
    - [x] The resolver's `resolve` method can be configured to return a specific DNS error.
    - [x] Modify `certwatch::app::run` to accept an optional, pre-built `Arc<dyn DnsResolver>`.
    - [x] Modify `TestAppBuilder` to allow injecting the `MockDnsResolver`.
    - [x] Write an integration test that injects the `MockDnsResolver`, processes a domain, and verifies the `cert_processing_failures` metric is incremented and a warning is logged.


### Epic: Eliminate Lock Contention in DNS Health Monitor

- **ID:** `#112`
- **Status:** Not Started
- **Description:** The `DnsHealthMonitor` currently uses a blocking `Mutex` to guard its internal state, causing contention on the hot path of recording DNS outcomes. This epic refactors the monitor to use a message-passing model, moving all state mutations to a dedicated background task and making the `record_outcome` method non-blocking.

---

- [x] **#113 - Phase 1: Write a Failing Test to Prove Lock Contention**
  - **Context:** Following TDD, we must first create a test that fails with the current implementation, proving the existence of the problem we intend to solve.
  - **Subtasks:**
    - [x] Create a new `#[tokio::test]` named `test_record_outcome_is_non_blocking` in `src/dns/health.rs`.
    - [x] In the test, acquire a lock on the `monitor_state` to simulate a long-running operation holding the lock.
    - [x] While the lock is held, spawn a separate task to call `record_outcome`.
    - [x] Use `tokio::time::timeout` to assert that the call to `record_outcome` blocks and eventually times out. This failing test is the goal.

- [x] **#114 - Phase 2: Implement Message-Passing Refactor**
  - **Context:** With a failing test, we will now refactor the `DnsHealthMonitor` to eliminate the lock contention.
  - **Subtasks:**
    - [x] Add a `tokio::sync::mpsc::UnboundedSender<bool>` to the `DnsHealthMonitor` struct.
    - [x] Create a private async `state_update_task` that runs in a loop, receiving outcomes from a channel and exclusively managing the `monitor_state` lock.
    - [x] Update `DnsHealthMonitor::new` to create the channel and spawn the `state_update_task`.
    - [x] Change `record_outcome` to be a single, non-blocking `self.outcome_tx.send(is_success)` call.

- [x] **#115 - Phase 3: Verify Fix and Ensure Robustness**
  - **Context:** After implementing the fix, we must verify that our original failing test now passes and add an additional test to ensure the new implementation is robust under load.
  - **Subtasks:**
    - [x] Run the `test_record_outcome_is_non_blocking` test and confirm that it now passes.
    - [x] Create a new `#[tokio::test]` named `test_health_monitor_concurrent_updates`.
    - [x] This test will spawn a large number of concurrent tasks that all call `record_outcome`.
    - [x] After the tasks complete, the test will assert that the final state inside the monitor is correct (e.g., the number of outcomes matches the number of calls), proving that no messages were dropped.

---
### Epic: Improve Unit Test Coverage
**User Story:** As a developer, I want comprehensive unit tests for core logic so that I can refactor with confidence and catch regressions early.

- [x] **#116 - Unit Test Pattern Matching Logic**
  - **Context:** Add unit tests for `src/matching.rs` to verify the `RegexMatcher` in isolation.
  - **Subtasks:**
    - [x] Test successful compilation of valid regex patterns.
    - [x] Test for errors on invalid regex patterns.
    - [x] Test simple, wildcard, and case-insensitive domain matches.
    - [x] Test non-matches and edge cases like empty inputs.

- [x] **#117 - Unit Test TSV Enrichment Logic**
  - **Context:** Add unit tests for `src/enrichment/tsv_lookup.rs`.
  - **Subtasks:**
    - [x] Test successful parsing and lookup from a valid in-memory TSV.
    - [x] Test lookup of non-existent keys.
    - [x] Test for errors on malformed TSV data (e.g., jagged rows).

- [x] **#118 - Unit Test Configuration Parsing**
  - **Context:** Add unit tests for `src/config.rs`.
  - **Subtasks:**
    - [x] Test parsing of a full, valid TOML configuration.
    - [x] Test that missing optional fields are populated with default values.
    - [x] Test for errors on invalid value types or missing required fields.
---
### Epic: Architectural Enhancements
**User Story:** As a developer, I want the application to be more observable, maintainable, and robust, so that I can easily diagnose issues, contribute new features, and ensure long-term stability.

- [x] **#119 - Enhanced Observability with `tracing`**
  - **Context:** Integrate the `tracing` ecosystem for structured, context-rich logs.
  - **Subtasks:**
    - [x] Add `tracing` and `tracing-subscriber` to `Cargo.toml`.
    - [x] Initialize the `tracing` subscriber in `src/main.rs`.
    - [x] Add `#[tracing::instrument]` to key functions like `app::process_domain`, `outputs::OutputManager::send_alert`, and `dns::DnsResolutionManager::resolve_with_retry`.

- [x] **#121 - Strategic Type Aliases for Readability**
  - **Context:** Simplify complex type signatures to improve code readability.
  - **Subtasks:**
    - [x] Create a new `src/types.rs` module.
    - [x] Define type aliases like `DomainReceiver` and `AlertSender`.
    - [x] Refactor function signatures in `src/app.rs` to use the new aliases.


---
### Epic 38: Improve Startup Resilience and Diagnostics
**User Story:** As an operator, I want the application to provide immediate, clear feedback on startup if a critical dependency like DNS is misconfigured, so I can diagnose and fix the issue without confusion.

- [x] **#123 - Prevent Hang on Unresponsive DNS Resolver**
  - **Context:** The application would hang silently on startup if the configured DNS resolver was unreachable, making it difficult to debug.
  - **Subtasks:**
    - [x] Implemented a pre-flight health check in `DnsHealthMonitor` to validate the DNS resolver on startup.
    - [x] Refactored the check to be mockable and accept a `&amp;dyn DnsResolver`.
    - [x] Integrated the check into `app::run` to ensure it runs before any other services.
    - [x] Added integration tests using a `FakeDnsResolver` to verify both successful and failing startup scenarios.
- [x] **#124 - Ensure Early Startup Logging**
  - **Context:** No log messages were visible until late in the startup process, making it hard to trace initialization issues.
  - **Subtasks:**
    - [x] Refactored `main.rs` to initialize the `tracing_subscriber` immediately after parsing CLI arguments.
    - [x] Ensured that configuration loading and pre-flight checks are now logged.
    - [x] Removed network-dependent checks from the configuration logging function.

---
### Epic 39: Improve Startup Resilience for Enrichment Data
**User Story:** As an operator, I want the application to fail fast with a clear error message if the configured TSV enrichment file is missing or invalid, so I can avoid silent failures and quickly diagnose configuration issues.

- [x] **#125 - Implement Pre-flight Check for TSV Enrichment Data**
  - **Context:** Following the "Startup Pattern", we will add a pre-flight check to validate the TSV enrichment data source at application startup.
  - **Subtasks:**
    - [x] Create a new integration test file `tests/enrichment_startup.rs`.
    - [x] Add integration tests to verify that the application exits with an error if the `asn_tsv_path` is configured but the file is missing or malformed.
    - [x] Add an integration test to verify that the application starts successfully if `asn_tsv_path` is not configured.
    - [x] Create a new module `src/enrichment/health.rs`.
    - [x] Implement a `startup_check(config: &Config) -> Result<Arc<dyn EnrichmentProvider>>` function in the new module.
    - [x] Refactor `main()` to call the new `startup_check` function.
    - [x] Refactor `app::run()` to accept the `EnrichmentProvider` as an argument, removing the internal creation logic.


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
  - [x] **#130 - Refactor `dns_failure.rs` to be a reliable unit test.**
  - [x] **#131 - Add `reset()` method to `TestMetrics` helper.**
  - [x] **#132 - Improve error propagation from `build_alert` function.**
  - [ ] **#133 - Fix flaky `test_enrichment_failure_increments_failure_metric` test.**
