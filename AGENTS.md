# Agent instructions

This is a public, unofficial Rust CLI for VUT's information system. Read `README.md` and `docs/architecture.md` before changing commands or API access. Keep the binary as the user-facing interface; add modules only when a real responsibility requires them.

- Use the Rust version pinned by `mise.toml` and `rust-toolchain.toml`; keep both aligned.
- Run `mise exec -- cargo fmt --all --check`, `mise exec -- cargo clippy --locked --all-targets -- -D warnings`, `mise exec -- cargo test --locked --all-targets`, and `mise exec -- cargo build --locked` before reporting code ready.
- Keep routine tests offline. Use anonymized fixtures and local mock HTTP; assert requests and JSON/error behavior, not only successful parsing.
- Treat `.env`, tokens, cookies, browser state, and actual VUT responses as private. Do not open `.env` without the user's permission, print secrets, or commit personal data. Access secrets through exported environment variables.
- Use documented endpoints and the current user's permissions. Live discovery and smoke tests are read-only unless the user explicitly authorizes a specific write action.
- Keep CLI-owned JSON and exit codes stable; label upstream pass-through JSON as raw. Record endpoint provenance and observed limitations beside implementation and tests.
