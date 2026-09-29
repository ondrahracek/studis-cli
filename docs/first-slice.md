# First slice plan: offline CLI foundation

This plan follows `AGENTS.md`, `README.md`, and `docs/architecture.md`. The baseline on `master` is a `studis` scaffold that always exits 2; all four Rust gates pass with zero tests. Three read-only plan reviewers examined scope, regressions, and likely review findings.

## Build order: RED–GREEN–REFACTOR

1. Add black-box process tests for `--help`, `--version`, `capabilities`, unknown commands, and missing subcommands. For `capabilities`, assert exact JSON fields and values: `schema_version: 1`, `cli_version` equal to the package version, and `commands: ["capabilities"]`. Assert successful commands leave stderr empty; parse failures exit 2, leave stdout byte-empty, and explain the error on stderr. Clap handles help/version immediately, so trailing tokens after them are outside this parse-error contract. Scrub inherited credential variables from child processes, then separately inject harmless sentinel values to prove they do not affect output. Run the tests against the scaffold and record the predicted red failures.
2. Add `clap` derive parsing with `--help`, `--version`, and a single `capabilities` subcommand. Expose the library's `cli` module to the binary and keep `main.rs` thin. Use Clap's standard parse error and exit behavior; disable color for deterministic output. Keep the command list static because only one executable subcommand exists.
3. Emit one CLI-owned JSON object on stdout for `capabilities`. Use `serde` and `serde_json` for a typed output model. No input or account data is read. Make the new tests green, then refactor without changing their observations.
4. Replace the scaffold status in README with usage examples, exact JSON fields/types, and exit/stream behavior. Run `mise exec -- cargo fmt --all --check`, `mise exec -- cargo clippy --locked --all-targets -- -D warnings`, `mise exec -- cargo test --locked --all-targets`, and `mise exec -- cargo build --locked`. Baseline is 0 tests; completion requires real process tests with observed red and green phases.

## Cut, and why

- Command registry: `rg` finds no command-discovery caller, and `capabilities` is the only implemented command. A registry duplicates Clap declaration now; revisit when a second command makes drift a real risk.
- `--format text`: `--help` serves the human-facing use case; no text-output consumer exists. Add only when a real command needs it.
- Custom error and exit machinery: Clap's parse errors already provide exit 2 and stderr diagnostics; the tests will pin that behavior.
- Local hook and GitHub CI execution as separate slice requirements: hooks repeat the four required local gates; CI runs when work is published. Publishing is a separate action.
- Plugin system, configuration hierarchy, release packaging, additional Cargo crates, OAuth, HTTP, VUT endpoints, live account fixtures, and write commands: none is required for an offline one-command foundation.

## Open question

The JSON command list is intentionally static. A second command should trigger a review of discovery design before expanding the list.

## TDD and review evidence

- RED: with tests written before production changes, `cargo test --locked --test cli` failed 4 of 5 cases against the scaffold. The missing-command case already passed because the scaffold also exited 2; it was retained as a guard for the parser.
- GREEN: after implementation, the offline process suite passed all 6 cases. The four required Rust gates passed.
- ATTACK wave 1: a test-quality reviewer mutated `cli_version` to use `VUT_API_CLIENT_SECRET` when set; the original suite still passed. A sentinel-credential assertion was added, and that same mutation then made `capabilities_has_exact_versioned_json_contract` fail. A correctness review confirmed Clap's immediate help/version behavior, which the README now states. The conventions review found no substantive issue.
- ATTACK wave 2: a closed stdout pipe produced a Rust panic and exit 101. A new process test failed against that code; explicit output error handling made it pass. Another review found that Clap echoes invalid argv in diagnostics, so the README now scopes its account-data guarantee to `capabilities` output.
- ATTACK wave 3: a reviewer found the documented trailing newline was not asserted. A byte-level assertion was added; replacing `writeln!` with `write!` made that named test fail, then restoring `writeln!` made it pass.
