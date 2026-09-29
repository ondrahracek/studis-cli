# Architecture

The executable is the only planned product interface. `main.rs` handles process startup and exit codes. `lib.rs` holds internal modules so command behavior can be tested without driving a real VUT account. `cli` owns argument parsing and CLI-controlled JSON/error shapes. `auth` acquires per-user OAuth client-credentials tokens. `http` owns requests, timeouts, status translation, and redaction. `resources` contains one module per VUT resource, with API wire types separate from CLI output models.

This tree is a boundary map, not an API implementation. The placeholder files contain no endpoint paths, requests, or credential handling. Keep a single Cargo package until a real external Rust consumer or independent build boundary warrants more crates.

The [first implementation slice](first-slice.md) establishes offline command parsing, output, and process tests. The following slice should establish one read-only API path with a documented endpoint, an anonymized response fixture, a local mock OAuth/API server, and a black-box CLI test. Then add studies, news, and schedule without hardcoding a student's study ID. Token renewal means a fresh client-credentials grant; it is not a refresh-token flow. Default tests and CI never contact VUT.

Before any public command claims coverage, verify its endpoint documentation, parameters, permission behavior, time semantics, and error cases. Raw VUT JSON can be exposed later under an explicit upstream-owned contract. Commands that change data require their own design and authorization boundary.
