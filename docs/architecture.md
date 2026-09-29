# Architecture

The executable is the user-facing interface. `main.rs` handles process startup and exit codes. `lib.rs` holds internal modules so command behavior can be tested without driving a real VUT account. `cli` owns argument parsing and CLI-controlled JSON/error shapes. `auth` builds the OAuth client-credentials request, parses its token response, and controls one 401 recovery for cached tokens. `token_store` stores access tokens in the operating system credential store. `http` configures the shared HTTP client and timeouts. `resources` contains endpoint-specific requests, response checks, and safe errors.

The [CLI foundation](cli-foundation.md) implements parsing, command discovery, and output behavior. `auth` uses an environment-provided Bearer token or reuses a cached client-credentials token until VUT returns 401; `http` executes read-only requests with fixed timeouts and no redirects. Resource modules implement [studies access](studies-access.md) and [news and schedule](news-and-schedule.md). `dates` validates CLI date inputs before credentials are read. The project is one Cargo package.

The current news and schedule endpoints use the authenticated person's context without a hardcoded study ID. Each endpoint needs its own documented behavior, offline request/response tests, and read-only live check. Token renewal means a fresh client-credentials grant; it is not a refresh-token flow. Default tests and CI never contact VUT.

Authenticated commands return upstream-owned JSON under `raw`. The implemented VUT operations are reads; endpoint provenance and observed limits are documented with each command.
