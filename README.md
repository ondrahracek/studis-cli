# studis-cli

An unofficial, student-maintained command-line interface for the Brno University of Technology (VUT) information system. The intended executable name is `studis`. It will provide predictable JSON output for coding agents and useful commands for people. Studis is the familiar student portal name; the eventual scope may include other VUT systems where documented APIs and user permissions allow it.

**Status:** project scaffold. The binary currently exits with an “unavailable” message. It does not authenticate or call VUT yet. This project is not affiliated with or endorsed by VUT.

## Development setup

Rust 1.98.1 is pinned in [`mise.toml`](mise.toml) and [`rust-toolchain.toml`](rust-toolchain.toml). If mise is installed, install the project toolchain with `mise install`, then run:

```sh
git config --local core.hooksPath .githooks
mise exec -- cargo fmt --all --check
mise exec -- cargo clippy --locked --all-targets -- -D warnings
mise exec -- cargo test --locked --all-targets
mise exec -- cargo build --locked
```

The tracked pre-commit hook applies rustfmt, stops if formatted files need staging, checks staged whitespace, and runs Clippy. The pre-push hook runs tests and a build. Git does not enable repository hooks automatically on clone; the `git config` command above enables them for this checkout. The same four Rust checks run on GitHub Actions for pull requests and pushes to `master`. The current scaffold has no behavior tests; tests will accompany the first implemented commands. There is no release or deployment workflow yet.

## Credentials and live API testing

The default build and test commands need no VUT account. Once API commands exist, each user will register their own VUT OAuth client and put its ID and secret in a local `.env`, using [`.env.example`](.env.example) as a list of variable names. `.env` is ignored by Git; [`.envrc`](.envrc) loads it through direnv. The current variable names are `VUT_API_CLIENT_UID` and `VUT_API_CLIENT_SECRET`. Never commit credentials, access tokens, cookies, personal responses, or recordings from an authenticated browser. Do not use a VUT password for API automation.

Live VUT checks will be opt-in and read-only by default. Documentation visibility does not prove that an OAuth client may call every endpoint. The authenticated API inventory used during research stays outside this public repository.

## Layout

- `src/main.rs`: executable entry and exit behavior
- `src/cli.rs`: command parsing and output contracts
- `src/auth.rs`: OAuth credential and token handling
- `src/http.rs`: HTTP transport, errors, and redaction
- `src/resources/`: endpoint-specific operations and wire types
- `tests/`: process and mock-HTTP tests as commands are implemented
- `docs/architecture.md`: module boundaries
- `docs/first-slice.md`: proposed offline CLI foundation

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The project is licensed under [MIT](LICENSE).
