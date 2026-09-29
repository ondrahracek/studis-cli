# CLI foundation: command and output contract

`studis` is the executable for the public `studis-cli` Rust package. `--help`, `--version`, and `capabilities` work offline; `studies list` makes a read-only VUT API request. The binary delegates to `src/cli.rs` through `src/main.rs`. The authenticated flow is documented in [studies access](studies-access.md).

## Current behavior

`studis capabilities` writes one CLI-owned JSON object followed by a newline:

```json
{"schema_version":1,"cli_version":"0.0.0","commands":["capabilities","studies list"]}
```

`schema_version` is an integer for the JSON contract, `cli_version` is the package version string, and `commands` lists executable command paths in this build. The list is static and tested against the supported paths; review discovery design if it becomes difficult to keep aligned with dispatch. Clap's implicit `help` subcommand is disabled; `--help` remains available.

Successful commands exit 0 and leave stderr empty. Clap parse errors, including a missing or unknown command, exit 2 with empty stdout and a diagnostic on stderr. Help and version are immediate Clap actions, so they may succeed even with trailing tokens. Color is disabled. A closed stdout pipe exits successfully without a panic; other output errors exit 1 with a diagnostic. `capabilities` does not read credentials or account data. Clap can echo invalid argument text in diagnostics, so callers must not put secrets in argv.

## Design choices

The package remains one Cargo crate. `clap` derive owns parsing; `serde` and `serde_json` own the typed JSON model; `assert_cmd` supports black-box process tests. A registry, text format, custom parse-error framework, configuration hierarchy, and plugin system would add duplicate or unused machinery at the current command count. These choices should be revisited when a real command or consumer needs them; avoid extending the foundation spec by habit.

CLI-owned JSON is versioned here. The studies command places complete upstream VUT JSON under `raw`; its nested fields are outside this stability promise. API behavior is tested offline through pure request and parser tests and checked read-only against VUT, without a local imitation of VUT's API.

## Verification record

The command suite was developed RED–GREEN–REFACTOR: four of five initial process tests failed against the scaffold as predicted; the missing-command test already passed because the scaffold also exited 2. The implemented parser and command made those tests green. Review waves then found a credential-environment blind spot, a broken-pipe panic, and an untested JSON newline. Harmless credential sentinels and targeted output mutations proved the new assertions can catch each defect. The foundation landed with seven offline process tests in `tests/cli.rs`. The repository gates are `cargo fmt --all --check`, Clippy with `-D warnings`, `cargo test --locked --all-targets`, and `cargo build --locked` under the pinned Rust toolchain.
