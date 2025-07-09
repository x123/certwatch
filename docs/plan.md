---
### Epic 37: Unplanned Maintenance & Bug Fixes
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
    - [x] Refactored the check to be mockable and accept a `&dyn DnsResolver`.
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


---
---

# Feature Implementation Plan: Batched Slack Notifications

This document outlines the detailed, incremental plan for implementing a robust, batched Slack notification system. The core architectural choice is to use an **Event Bus (Publisher/Subscriber)** model to decouple alert generation from the notification logic. This is necessary to handle the stateful, asynchronous nature of batching alerts over a time window.

The implementation is broken down into two sequential epics:

1.  **Epic #42: Core Notification Pipeline:** First, we will build the fundamental pub/sub architecture using a broadcast channel. This creates the backbone of the system and will be tested with a simple logging subscriber.
2.  **Epic #43: Slack Batching and Delivery:** Finally, we will build the dedicated Slack notification service that subscribes to the event bus and handles the complex logic of batching and sending alerts.

---

### Epic #42: Core Notification Pipeline

-   **User Story:** As a developer, I want to implement a decoupled event pipeline that allows the core application to publish alerts without being directly tied to the notification logic.
-   **Why:** This architectural change is the most critical part of the feature. It isolates the core "business logic" of finding certificates from the "side-effect" of sending notifications. This makes the system more modular, easier to test, and extensible for future notification channels. We will validate the pipeline with a simple logging subscriber before adding the complexity of Slack.
-   **Architecture:**
    ```mermaid
    graph TD
        subgraph app.rs
            A[process_domain] --> B{Build Alert};
            B --> C[event_bus.send(alert)];
        end

        subgraph main.rs
            D[main] --> E{Create Event Bus};
            E --> C;
            E --> F[Spawn Logging Subscriber];
        end

        subgraph logging_subscriber.rs
            F --> G{Listen on Event Bus};
            G --> H[log::info!("New Alert: ...")];
        end
    ```
-   **Acceptance Criteria:**
    -   An application-wide `tokio::sync::broadcast` channel for `Alert`s is created in `main.rs`.
    -   The `app::run` function publishes each new `Alert` to this channel.
    -   A new, simple "logging subscriber" is created that listens for alerts and logs them to the console.
    -   The entire pipeline is wrapped in a configuration flag (`--enable-notifications`) and is disabled by default.
    -   Integration tests verify that an alert generated by the app is successfully received and logged by the subscriber.

-   **Tasks:**
    -   [x] **#134 - Add `notification` Module:** Create a new `src/notification` module with `mod.rs`.
    -   [x] **#135 - Define `Alert` Event Bus in `main`:** In `main.rs`, create a `tokio::sync::broadcast::channel<Alert>`.
    -   [x] **#136 - Publish Alerts from `app::run`:** Pass the `Sender` end of the channel to `app::run` and call `sender.send(alert)` after an alert is created. Handle potential send errors by logging them.
    -   [x] **#137 - Create `LoggingSubscriber`:** In the `notification` module, create a `LoggingSubscriber` task that takes a `Receiver`, loops, and logs each received alert.
    -   [x] **#138 - Add Configuration:** Add `--enable-notifications` flag and `CERTWATCH_ENABLE_NOTIFICATIONS` env var. In `main.rs`, only spawn the subscriber and pass the sender to `app::run` if this is enabled.
    -   [x] **#139 - Write Pipeline Unit Test:** Create a new unit test that enables notifications, runs the app, and asserts that the expected alert information appears in the logs.

---

### Epic #43: Slack Batching and Delivery

-   **User Story:** As a security operator, I want alerts to be collected and sent to Slack in batches to avoid flooding our channels, with the batching configurable by size and time.
-   **Why:** This epic delivers the final, user-facing feature. It builds on the robust, tested pipeline from the previous epics. All Slack-specific logic, including HTTP requests, JSON serialization, and the complex batching mechanism, is completely isolated within this new `NotificationManager`.
-   **Architecture:**
    ```mermaid
    graph TD
        subgraph main.rs
            A[main] --> B{If Slack URL is set};
            B --> C[Spawn NotificationManager];
        end

        subgraph notification_manager.rs
            C --> D[Subscribe to Event Bus];
            D --> E{Loop + select!};
            E -- "On Alert" --> F[Add to Batch];
            E -- "On Timer" --> G[Send Batch];
            E -- "On Batch Full" --> G;
            G --> H[Use SlackClient];
        end

        subgraph slack_client.rs
            H --> I[Build JSON Payload];
            I --> J[POST to Webhook];
        end
    ```
-   **Acceptance Criteria:**
    -   A new `NotificationManager` subscribes to the `Alert` event bus.
    -   The manager buffers alerts.
    -   The buffer is flushed and sent to Slack when either a size limit (`--slack-batch-size`) or a time limit (`--slack-batch-timeout`) is reached.
    -   The Slack message is a single, richly-formatted message containing all alerts in the batch.
    -   All HTTP interactions are robust, with timeouts and error handling.
    -   Failures to send to Slack are logged and tracked via a new `slack_notification_failures` metric.
    -   The entire feature is only active if a `--slack-webhook-url` is provided.

-   **Tasks:**
    -   [x] **#140 - Add Dependencies:** Add `reqwest` and `serde_json` to `Cargo.toml`.
    -   [x] **#141 - Add Slack Configuration:** Add `--slack-webhook-url`, `--slack-batch-size`, and `--slack-batch-timeout` arguments and corresponding config entries.
    -   [x] **#141b - Refactor Slack Configuration:** Co-located all Slack settings under `[output.slack]` for clarity, including a new `enabled` flag. Removed the confusing top-level `[notifications]` table.
    -   [x] **#142 - Implement `SlackMessage` Payload:** In a new `src/notification/slack.rs`, define the structs for the Slack message payload (`Attachment`, `Field`, etc.) and a function that serializes a `&[Alert]` into the correct JSON. Write unit tests for this serialization logic.
    -   [x] **#143 - Implement `SlackClient`:** Create a `SlackClient` struct that holds a `reqwest::Client` and the webhook URL. Give it a `send_batch(&self, alerts: &[Alert])` method.
    -   [x] **#144 - Test `SlackClient`:** Unit test the `SlackClient` using `wiremock-rs` (https://docs.rs/wiremock/latest/wiremock/ , latest version is 0.6.4). Test for successful posts, 500 errors, and network timeouts.
    -   [x] **#145 - Implement `NotificationManager`:** Create the `NotificationManager` task. Implement the core `select!` loop for batching by size and time.
    -   [x] **#146 - Test `NotificationManager` Batching:** Unit test the `NotificationManager`'s logic. Give it a `FakeSlackClient`. Test that it sends a batch when the size limit is hit. Use `tokio::time::pause/advance` to test that it sends a batch when the timeout is hit.
    -   [x] **#147 - Integrate into `main.rs`:** In `main.rs`, if the Slack webhook URL is provided, spawn the `NotificationManager` and pass it a receiver from the event bus.

---

### Epic #44: Retroactive - Improve Test Suite Robustness
- **User Story:** As a developer, I want the test suite to be fast, reliable, and deterministic, so that I can refactor with confidence and trust the CI/CD pipeline.
- **Why:** A series of flaky, timeout-prone integration tests were causing CI failures and slowing down development. This effort systematically replaced them with focused, reliable unit tests, codifying a new, more robust testing philosophy for the project.
- **Tasks:**
   - [x] **#148 - Refactor `shutdown_integration.rs`:** Replaced a slow, sleep-based end-to-end test with a fast unit test that directly verifies the `tokio::sync::watch` shutdown signal propagation.
   - [x] **#149 - Refactor `bounded_concurrency.rs`:** Replaced a `sleep`-based test with a deterministic test using a `tokio::sync::Barrier` to control and verify concurrency limits.
   - [x] **#150 - Refactor Hot-Reload Tests:** Deleted the complex `live_hot_reload.rs` and refactored `pattern_hot_reload.rs` to use focused unit tests that call the `reload()` method directly, removing dependencies on filesystem events.
   - [x] **#151 - Refactor `certstream_client.rs`:** Replaced timeout-based synchronization with a deterministic `oneshot` channel to signal completion, making the tests faster and more reliable.

---

### Epic #45: Enhance Slack Notification Scannability

- **User Story:** As a security analyst, I want the alerts within a single Slack notification batch to be sorted using a multi-level pipeline: first by base domain, then by the full domain name (FQDN), and finally by the Autonomous System Number (ASN), so that I can see a highly organized report where related domains and networks are grouped together, allowing for faster analysis and identification of potentially coordinated activity.
- **Why:** Raw, unsorted alert batches are difficult to parse, especially when they contain related domains. A structured, sorted report allows analysts to quickly identify patterns (e.g., multiple subdomains from the same parent domain, or multiple domains from the same ASN) and prioritize their focus.
- **Architecture:** The sorting will be implemented using the **Decorate-Sort-Undecorate** pattern for performance and clarity. This logic will reside entirely within the `SlackTextFormatter` in `src/formatting.rs`, as sorting is a presentation-layer concern.
  ```mermaid
  graph TD
      subgraph Decorate
          A[Batch of Alerts: Vec<Alert>] --> B{For each Alert...};
          B --> C[Extract Base Domain, FQDN, ASN];
          C --> D[Create SortableAlert struct];
      end
      subgraph Sort
          E[Vec<SortableAlert>] --> F{sort() method};
          F --> G[Sorted Vec<SortableAlert>];
      end
      subgraph Undecorate
          G --> H[Extract original Alerts in order];
          H --> I[Format into single String];
      end
      D --> E;
  ```
- **Acceptance Criteria:**
    -   Alerts in a Slack batch are sorted using a three-level, stable, case-insensitive comparison.
    -   **Primary Key:** Registrable domain (e.g., `example.com`).
    -   **Secondary Key:** Full domain name (e.g., `a.example.com`).
    -   **Tertiary Key:** ASN organization name.
    -   Alerts with unparsable domains are placed at the very end of the batch.
    -   Alerts with no ASN information are placed at the end of their respective FQDN group.
- **Tasks:**
    - [x] **#152 - Prerequisite: Fix VirusTotal Link:** Modify the VirusTotal link generation in `src/formatting.rs` to use only a single IP address, per prior feedback.
    - [x] **#153 - Implement `SortableAlert` Struct:** In `src/formatting.rs`, create a private `SortableAlert` struct to hold pre-calculated sort keys (`base_domain`, `fqdn`, `asn_org`) and a reference to the original `Alert`.
    - [x] **#154 - Implement `Ord` for `SortableAlert`:** Implement the `Ord` trait for `SortableAlert` to define the multi-level comparison logic, correctly handling `None` values for optional keys.
    - [x] **#155 - Refactor `format_batch`:** Update `SlackTextFormatter::format_batch` to use the Decorate-Sort-Undecorate pattern: map to `Vec<SortableAlert>`, sort the vector, then map back to the original alerts for formatting.
    - [x] **#156 - Add Sorting Unit Test:** Create a new unit test `test_format_batch_is_correctly_sorted` that verifies the complete sorting logic, including all sorting levels and edge cases for missing data.


---

## Epic #46: Implement Advanced, Staged Rule-Based Filtering Engine

**As a** Security Analyst,
**I want to** define complex, multi-stage filtering rules that combine various data points using boolean logic,
**So that** I can create highly specific alerts, reduce false positives, and minimize expensive operations like external API calls.

---

### **Phase 1: Foundational Two-Stage Rule Engine**
*   **Goal:** Implement the core filtering logic for domain, ASN, and IP criteria using an efficient two-stage pipeline.
*   **Tasks:**
    *   [x] **#157 - Config Schema:** In `config.yml`, add a `rule_files` key to load multiple rule YAML files.
    *   [x] **#158 - Rule Data Models:** Create Rust structs for `Rule`, `RuleExpression`, etc., with `serde` support. The initial schema will support `domain_regex`, `asns`, `ip_networks`, `not_asns`, `not_ip_networks`, and a top-level `all` for AND logic.
    *   [x] **#159 - Rule Loading & Validation:** Implement logic to load all specified rule files at startup, validate the syntax of each rule (valid regex, CIDR, etc.), and compile them into the internal struct format.
    *   [x] **#160 - Two-Stage Classifier:** Implement the startup logic that automatically sorts rules into a "Stage 1" (regex-only) or "Stage 2" (enrichment-required) collection.
    *   [x] **#161 - Staged Evaluator:** Implement the `evaluate` function and integrate it into the main pipeline. It must first check Stage 1 rules, trigger enrichment if there's a match, and then check Stage 2 rules.
    *   [x] **#162 - Unit & Integration Tests:** Add comprehensive tests for rule parsing, validation, and the two-stage evaluation logic.

---

### **Phase 2: Refactor for Concurrency and Hot-Reloading**
*   **Goal:** Resolve a critical concurrency bottleneck in the rule evaluation pipeline and refactor the architecture to support atomic, lock-free updates for future hot-reloading capabilities.
*   **Tasks:**
    *   [ ] **#172 - Diagnose Contention:** Add targeted `tracing` spans to the worker loop and `RuleMatcher` to confirm the exact source of contention under load.
    *   [ ] **#173 - Stateless `RuleMatcher`:** Refactor the `RuleMatcher` and its associated `Condition`s to be completely stateless, ensuring the `match` method only requires an immutable `&self` reference.
    *   [ ] **#174 - Implement `watch` Channel for Rules:** Replace the simple `Arc<RuleMatcher>` with a `tokio::sync::watch` channel to allow the central rule set to be updated atomically without interrupting worker threads. This makes future hot-reloading possible.
    *   [ ] **#175 - Verify Fix:** Write a new integration test that spawns multiple workers and feeds a high volume of domains, asserting that processing is not serialized and throughput is maintained when rules are active.

---

### **Phase 3: Advanced Boolean Logic**
*   **Goal:** Enhance the rule engine's expressiveness by adding support for `any` (OR) and nested conditions.
*   **Tasks:**
    *   [ ] **#176 - Update Schema & Parsing:** Extend the YAML schema and `serde` parsing to support `any` blocks and nested `all`/`any` structures.
    *   [ ] **#177 - Recursive Evaluator:** Refactor the `evaluate` function to be fully recursive, allowing it to walk the nested expression tree correctly.
    *   [ ] **#178 - Complex Logic Tests:** Add new unit tests specifically for verifying complex boolean scenarios (e.g., `all` nested inside `any`).

---

### **Phase 4: Formalize the Extensible Enrichment Framework**
*   **Goal:** Refactor the internal architecture to be a generic, multi-level pipeline, preparing for future high-cost enrichment stages.
*   **Tasks:**
    *   [ ] **#179 - EnrichmentLevel Enum:** Create a formal `EnrichmentLevel` enum (e.g., `Level0`, `Level1`, `Level2`).
    *   [ ] **#180 - Condition Trait:** Define a trait or method for rule conditions to report their required `EnrichmentLevel`.
    *   [ ] **#181 - Generic Pipeline:** Refactor the hardcoded two-stage pipeline into a generic loop that progresses through enrichment levels, filtering at each step. This is primarily an internal code quality improvement.

---

### **Phase 5: Add a New, High-Cost Enrichment Stage (Proof of Concept)**
*   **Goal:** Prove the framework's extensibility by adding a new, expensive check.
*   **Tasks:**
    *   [ ] **#182 - Define New Condition:** Add a hypothetical `http_body_matches` condition and assign it a new, higher `EnrichmentLevel`.
    *   [ ] **#183 - Implement Enrichment Provider:** Create a new enrichment provider that can perform the HTTP request.
    *   [ ] **#184 - Integration Test:** Create an integration test demonstrating that the new provider is only called for alerts that have successfully passed all lower-level filter stages.
