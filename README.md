# studis-cli

An unofficial, student-maintained command-line interface for the Brno University of Technology (VUT) information system. The intended executable name is `studis`. It will provide predictable JSON output for coding agents and useful commands for people. Studis is the familiar student portal name; the eventual scope may include other VUT systems where documented APIs and user permissions allow it.

**Status:** early read-only CLI. The binary supports help, version, command discovery, studies, study news, and personal teaching schedule reads through the VUT API. This project is not affiliated with or endorsed by VUT.

## Commands and output

```sh
studis --help
studis --version
studis capabilities
studis studies list
studis news list --since 2026-09-01
studis schedule teaching --from 2026-09-29T08:00 --to 2026-09-29T18:00
studis schedule weeks --from 2026-09-28 --to 2026-10-04
```

`studis capabilities` writes one JSON object to stdout:

```json
{"schema_version":1,"cli_version":"0.0.0","commands":["capabilities","studies list","news list","schedule teaching","schedule weeks"]}
```

`schema_version` is an integer for this CLI-owned JSON contract; `cli_version` is the package version string; `commands` contains executable subcommand names. The output ends with a newline. Help and version print text on stdout. Clap handles help and version immediately, even if tokens follow them. Other parse errors, including a missing or unknown command, exit 2 with an explanation on stderr and empty stdout. Successful invocations exit 0 with empty stderr. `capabilities` output has no color codes or account-specific data. Clap diagnostics can echo invalid arguments, so never put secrets in command-line arguments. A closed output pipe exits successfully without a panic.

`studis studies list` writes `{"schema_version":1,"raw":...}` followed by a newline. `raw` contains the complete VUT JSON response and is upstream-owned; its nested fields may change independently of this CLI's schema version. The command requires a user-owned VUT OAuth client or an access token. Missing credentials or an API failure exits 1 with empty stdout and a short redacted diagnostic on stderr. The command performs a GET; it has no VUT data write action.

The news and schedule commands use the same raw JSON wrapper. `--since` and schedule-weeks bounds are calendar dates (`YYYY-MM-DD`); teaching bounds are local date-times (`YYYY-MM-DDTHH:MM`) passed to VUT without timezone conversion. Invalid dates and reversed windows exit 2 before authentication. See [news and schedule](docs/news-and-schedule.md) for exact endpoint mappings and known limits.

## Development setup

Rust 1.98.1 is pinned in [`mise.toml`](mise.toml) and [`rust-toolchain.toml`](rust-toolchain.toml). If mise is installed, install the project toolchain with `mise install`, then run:

```sh
git config --local core.hooksPath .githooks
mise exec -- cargo fmt --all --check
mise exec -- cargo clippy --locked --all-targets -- -D warnings
mise exec -- cargo test --locked --all-targets
mise exec -- cargo build --locked
```

The tracked pre-commit hook applies rustfmt, stops if formatted files need staging, checks staged whitespace, and runs Clippy. The pre-push hook runs tests and a build. Git does not enable repository hooks automatically on clone; the `git config` command above enables them for this checkout. The same four Rust checks run on Ubuntu, macOS, and Windows in GitHub Actions for pull requests and pushes to `master`. The process tests run offline and require no VUT account. There is no release or deployment workflow yet.

## Credentials and live API testing

The default build and test commands need no VUT account. To use authenticated commands, [register a personal VUT API client](https://doc.vut.cz/cs/UzivatelskeUctyPrukazyIdentita/PristupAPI) and put its UID and secret in a local `.env`, using [`.env.example`](.env.example) as a list of variable names. `.env` is ignored by Git; [`.envrc`](.envrc) loads it through direnv. The variables are `VUT_API_CLIENT_UID` and `VUT_API_CLIENT_SECRET`. Never commit credentials, access tokens, cookies, personal responses, or recordings from an authenticated browser. Do not use a VUT password for API automation.

With client UID and secret, the CLI caches its access token by client UID. On macOS and Linux it uses a prompt-free private file:

- macOS: `~/Library/Application Support/studis-cli/token-cache.json`
- Linux: `${XDG_STATE_HOME:-$HOME/.local/state}/studis-cli/token-cache.json`

The Unix cache directory is mode `0700`; the cache and coordination lock are regular files with mode `0600`. The CLI requires those objects to be owned by the effective user and rejects unsafe permissions, symlinks, hard-linked cache files, and nonregular files. On macOS it also rejects extended ACLs. Cache replacement is atomic, and concurrent cache misses or cached-token 401 responses coordinate through a bounded lock so only one process normally requests a replacement token. The file contains only a format version and access tokens keyed by client UID. It is plaintext, so processes running as the same operating-system user and system administrators can read it; the tradeoff is unattended access without Keychain or Secret Service prompts, including after rebuilding the CLI.

Windows continues to use Windows Credential Manager. The offline Windows CI suite verifies synthetic cross-process write/read behavior and deletes its test entry afterwards.

The CLI reuses a cached token until a VUT GET returns HTTP 401, then obtains one new client-credentials token and retries that GET once. It does not renew based on the token's reported lifetime. A newly obtained token that receives 401 is not obtained again during that invocation. Unsafe, unavailable, or lock-contended storage is an error; there is no less-restricted fallback.

Upgrading from a version that used macOS Keychain or Linux Secret Service does not read, migrate, or remove the old entry. The first client-credentials request after upgrading therefore obtains a new token and can fail if VUT authentication is unavailable or rate limited; retry when the service is available. On macOS, the unused item named `studis-cli-vut-access-token` can be removed manually in Keychain Access if desired.

An already-issued Bearer token can instead be supplied as `VUT_API_ACCESS_TOKEN` in the environment. A nonempty token takes precedence over the client UID/secret, bypasses the CLI-managed token cache, and is never renewed or persisted by the CLI. Omit the variable when using client credentials. An expired or unauthorized supplied token causes the VUT GET to fail. Keep it private like the client secret.

Live VUT checks are opt-in and read-only. Documentation visibility does not prove that every OAuth client may call every endpoint. The authenticated API inventory used during research stays outside this public repository. The studies command was checked against one client and observed response shape; this does not establish universal permissions or pagination behavior.

## Layout

- `src/main.rs`: executable entry and exit behavior
- `src/cli.rs`: command parsing and output contracts
- `src/auth.rs`: OAuth credential and token handling
- `src/token_store.rs`: private Unix file cache and Windows Credential Manager storage
- `src/http.rs`: bounded HTTP client settings
- `src/resources/`: endpoint-specific operations and wire types
- `tests/`: offline process tests; pure request/response tests live beside source
- `docs/architecture.md`: module boundaries
- `docs/cli-foundation.md`: current command and output contract
- `docs/studies-access.md`: authenticated studies flow and upstream JSON boundary
- `docs/news-and-schedule.md`: news and schedule commands, endpoint mappings, and limits

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The project is licensed under [MIT](LICENSE).
