# Test layout

`cli.rs` contains black-box process tests for the offline command and output contract. Put small, anonymized API fixtures under `fixtures/` when API commands exist. Mock OAuth and VUT HTTP locally; assert method, path, query, headers, response decoding, stdout, stderr, and exit status. Routine `cargo test` must not contact VUT or require credentials. Keep live read-only smoke tests separate and opt-in.
