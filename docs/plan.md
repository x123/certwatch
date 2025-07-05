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
### **Epic 5: Output & Alerting**
This phase builds the flexible output system for delivering alerts.

- [ ] **#8 - Implement the Output Management System**
  - **Context:** Build the `OutputManager` and the specific output formatters. The entire system will be built on the `Output` trait.
  - **Dependencies:** #1
  - **Subtasks:**
    - [ ] Write unit tests for each output formatter (JSON, stdout, Slack). The tests for Slack will use a fake HTTP client injected as a dependency.
    - [ ] Implement `JsonOutput`, `StdoutOutput`, and `SlackOutput` structs, each fulfilling the `trait Output`.
    - [ ] Implement the `OutputManager` which holds a `Vec<Box<dyn Output>>` and dispatches alerts accordingly.

- [ ] **#9 - Implement Deduplication**
  - **Context:** Build the deduplication service that filters alerts based on the defined unique keys.
  - **Dependencies:** #8
  - **Subtasks:**
    - [ ] Write unit tests to verify the deduplication logic. Assert that a second alert with the same key is dropped, but an alert with the unique "First Resolution" key is processed.
    - [ ] Implement the `Deduplicator` service using a time-aware cache (e.g., `moka`) and the specified hashing logic for keys.

---
### **Epic 6: Finalization & Integration**
This final phase ties all the decoupled components together into a running application.

- [ ] **#10 - Implement Configuration Management**
  - **Context:** Load all application settings from a file and command-line arguments, making them available to the rest of the application.
  - **Dependencies:** #1
  - **Subtasks:**
    - [ ] Define a single, comprehensive `Config` struct using `serde`.
    - [ ] Write a unit test to verify that loading a sample TOML file correctly populates the `Config` struct.
    - [ ] Implement the configuration loading logic using a crate like `figment` to merge file-based settings with CLI arguments.

- [ ] **#11 - Wire All Components in `main.rs`**
  - **Context:** Write the main application function that initializes all concrete service implementations, injects them into the components that depend on their traits, and starts all concurrent tasks.
  - **Dependencies:** #3, #5, #7, #8, #9, #10
  - **Subtasks:**
    - [ ] In `main`, load the configuration from task #10.
    - [ ] Instantiate the concrete types: `RegexMatcher`, `TrustDnsResolver`, `MaxmindAsnLookup`, `OutputManager`, etc.
    - [ ] Create the `mpsc` channels that will connect the pipeline stages.
    - [ ] "Wire" the application by passing the concrete instances (as `Box<dyn Trait>`) into the constructors of the services that need them.
    - [ ] Spawn a `tokio` task for each concurrent process (client, matcher, resolver pool, etc.) and start the runtime.
