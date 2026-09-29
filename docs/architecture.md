# Architecture

The executable is the only planned product interface. `main.rs` handles process startup and exit codes. `lib.rs` holds internal modules so command behavior can be tested without driving a real VUT account. `cli` owns argument parsing and CLI-controlled JSON/error shapes. `auth` builds the OAuth client-credentials request and parses its token response. `http` configures the shared HTTP client and timeouts. `resources` contains endpoint-specific requests, response checks, and safe errors.

The [CLI foundation](cli-foundation.md) implements parsing, command discovery, and output behavior. `auth`, `http`, and `resources::studies` implement the first read-only [studies access](studies-access.md) flow; news and schedule remain placeholders. Keep a single Cargo package until a real external Rust consumer or independent build boundary warrants more crates.

Later commands can add news and schedule without hardcoding a student's study ID. Each endpoint needs its own documented behavior, offline request/response tests, and read-only live check. Token renewal means a fresh client-credentials grant; it is not a refresh-token flow. Default tests and CI never contact VUT.

Before any public command claims coverage, verify its endpoint documentation, parameters, permission behavior, time semantics, and error cases. Raw VUT JSON can be exposed later under an explicit upstream-owned contract. Commands that change data require their own design and authorization boundary.
