# First implementation slice: offline CLI foundation

Build the command and output contract before adding VUT authentication or HTTP. The executable remains `studis`; the first useful command is `studis capabilities`, which reports the commands this build actually supports. The list must come from one command registry so help and capability output cannot drift.

## Behavior

- `studis --help` and `studis --version` work without configuration or network access.
- `studis capabilities` emits one JSON object on stdout. `studis capabilities --format text` provides a concise human view; JSON is the default for agent use.
- The JSON envelope has a schema version, CLI version, and a command list with stable names and an `available` flag. It advertises only implemented behavior; planned VUT commands are absent.
- Invalid arguments fail with a documented nonzero exit code, a concise diagnostic on stderr, and no partial JSON on stdout. `--help` and `--version` exit successfully.
- Output is deterministic and noninteractive: no login attempt, browser, pager, color codes, telemetry, or user-specific values.

## Structure and packages

Use `clap` with derive for argument parsing, help, and version. Use `serde` and `serde_json` for owned JSON models. Keep process-facing dispatch thin in `main.rs`; put command behavior behind a testable function in `cli.rs`. Use `assert_cmd` for process-level tests. Add dependencies only with the behavior that uses them and commit the resulting `Cargo.lock`. The existing one-package layout is sufficient.

## Tests and completion

Test help, version, default JSON schema and values, text format, invalid arguments, exit codes, stdout/stderr separation, and operation without `.env` or network. Parse JSON in tests rather than matching its whitespace. Run the repository's format, Clippy, test, and build gates. Update README examples and record the public CLI-owned output contract before a later API slice consumes it.

The next slice can add OAuth client credentials and one documented read-only endpoint using a local mock server and anonymized fixtures. That later slice must keep API wire JSON separate from CLI-owned JSON.
