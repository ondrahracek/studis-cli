# Contributing

Issues and pull requests are welcome. Please describe the VUT behavior or documentation that supports a proposed command, including its permissions and whether it changes data. Keep live account data and credentials out of issues, commits, fixtures, and CI logs.

Use the Rust version pinned in the repository. Enable the tracked hooks with `git config --local core.hooksPath .githooks`; they apply formatting and run local checks. Before opening a pull request, run the four commands in the README: format, Clippy, tests, and build. Add focused tests for new behavior using anonymized fixtures and local mock HTTP servers. Routine tests must not require network access to VUT or a personal account.

Do not add a command that changes VUT data without an explicit design for authorization, preview behavior, and failure recovery. Report security issues privately to the maintainers rather than in a public issue.
