# Test layout

Add black-box CLI tests here when commands exist. Put small, anonymized API fixtures under `fixtures/`. Mock OAuth and VUT HTTP locally; assert method, path, query, headers, response decoding, stdout, stderr, and exit status. Routine `cargo test` must not contact VUT or require credentials. Keep live read-only smoke tests separate and opt-in.
