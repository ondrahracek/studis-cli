# CLI foundation: command and output contract

`studis` is the executable for the public `studis-cli` Rust package. Its current commands work offline: `--help`, `--version`, and `capabilities`. No command authenticates or contacts VUT yet. The binary delegates to `src/cli.rs` through `src/main.rs`; `src/auth.rs`, `src/http.rs`, and `src/resources/` are reserved for behavior added with later commands.

## Current behavior

`studis capabilities` writes one CLI-owned JSON object followed by a newline:

```json
{"schema_version":1,"cli_version":"0.0.0","commands":["capabilities"]}
```

`schema_version` is an integer for the JSON contract, `cli_version` is the package version string, and `commands` lists executable subcommands in this build. The one-command list is static. Review discovery design when a second command is added so this list cannot drift from dispatch. Clap's implicit `help` subcommand is disabled; `--help` remains available.

Successful commands exit 0 and leave stderr empty. Clap parse errors, including a missing or unknown command, exit 2 with empty stdout and a diagnostic on stderr. Help and version are immediate Clap actions, so they may succeed even with trailing tokens. Color is disabled. A closed stdout pipe exits successfully without a panic; other output errors exit 1 with a diagnostic. `capabilities` does not read credentials or account data. Clap can echo invalid argument text in diagnostics, so callers must not put secrets in argv.

## Design choices

The package remains one Cargo crate. `clap` derive owns parsing; `serde` and `serde_json` own the typed JSON model; `assert_cmd` supports black-box process tests. A registry, text format, custom parse-error framework, configuration hierarchy, and plugin system would add duplicate or unused machinery at the current command count. These choices should be revisited when a real command or consumer needs them; avoid extending the foundation spec by habit.

CLI-owned JSON is versioned here. Future raw VUT responses must be explicitly labelled as upstream-owned and kept outside this stability promise. Authentication, HTTP, and endpoint behavior need separate specifications and offline mock-backed tests before implementation.

## Verification record

The command suite was developed RED–GREEN–REFACTOR: four of five initial process tests failed against the scaffold as predicted; the missing-command test already passed because the scaffold also exited 2. The implemented parser and command made those tests green. Review waves then found a credential-environment blind spot, a broken-pipe panic, and an untested JSON newline. Harmless credential sentinels and targeted output mutations proved the new assertions can catch each defect. The foundation landed with seven offline process tests in `tests/cli.rs`. The repository gates are `cargo fmt --all --check`, Clippy with `-D warnings`, `cargo test --locked --all-targets`, and `cargo build --locked` under the pinned Rust toolchain.
