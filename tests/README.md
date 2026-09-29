# Test layout

`cli.rs` contains black-box process tests for the offline command and output contract. Use small synthetic JSON inputs to test parsing, request construction, redaction, stdout, stderr, and exit status. Routine `cargo test` must not contact VUT or require credentials. Verify real HTTP behavior with separate opt-in read-only live checks; do not build a local imitation of VUT's API.
