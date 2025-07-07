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
