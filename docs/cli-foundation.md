# CLI foundation: command and output contract

`studis` is the executable for the public `studis-cli` Rust package. `--help`, `--version`, and `capabilities` work offline; studies, account, news, schedule, and subject-view commands make read-only VUT API requests. The binary delegates to `src/cli.rs` through `src/main.rs`. Authenticated flows are documented in [studies access](studies-access.md), [academic context reads](academic-context.md), [news and schedule](news-and-schedule.md), and [one-subject view](subject-view.md).

## Current behavior

`studis capabilities` writes one CLI-owned JSON object followed by a newline:

```json
{"schema_version":1,"cli_version":"0.0.0","commands":["capabilities","studies list","studies index","account roles","news list","schedule teaching","schedule weeks","schedule terms","subjects show","auth web login"]}
```

`schema_version` is an integer for the JSON contract, `cli_version` is the package version string, and `commands` lists executable command paths in this build. The list is static and tested against the supported paths. Clap's implicit `help` subcommand is disabled; `--help` remains available.

Successful noninteractive commands exit 0 and leave stderr empty. `auth web login` prints a sign-in instruction to stderr while the browser is open. Clap parse errors, including a missing or unknown command, exit 2 with empty stdout and a diagnostic on stderr. Help and version are immediate Clap actions, so they may succeed even with trailing tokens. Color is disabled. A closed stdout pipe exits successfully without a panic; other output errors exit 1 with a diagnostic. `capabilities` does not read credentials or account data. Clap can echo invalid argument text in diagnostics, so callers must not put secrets in argv.

## Design choices

The package is one Cargo crate. `clap` derive owns parsing; `serde` and `serde_json` own the JSON model; `assert_cmd` supports black-box process tests. The current CLI has no plugin system, persistent configuration, or alternate output format.

CLI-owned JSON is versioned here. The individual authenticated resource commands place complete upstream VUT JSON under `raw`; its nested fields are outside this stability promise. `subjects show` composes stable section/status/provenance fields while retaining source records inside those sections. API behavior is tested offline through pure request, parser and composition tests and checked read-only against VUT, without a local imitation of VUT's API.

## Verification record

The command suite was developed RED–GREEN–REFACTOR: four of five initial process tests failed against the scaffold as predicted; the missing-command test already passed because the scaffold also exited 2. The implemented parser and command made those tests green. Review waves then found a credential-environment blind spot, a broken-pipe panic, and an untested JSON newline. Harmless credential sentinels and targeted output mutations proved the new assertions can catch each defect. The foundation landed with seven offline process tests in `tests/cli.rs`. The repository gates are `cargo fmt --all --check`, Clippy with `-D warnings`, `cargo test --locked --all-targets`, and `cargo build --locked` under the pinned Rust toolchain.
