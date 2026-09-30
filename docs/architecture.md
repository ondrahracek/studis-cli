# Architecture

The executable is the user-facing interface. `main.rs` handles process startup and exit codes. `lib.rs` holds internal modules so command behavior can be tested without driving a real VUT account. `cli` owns argument parsing and CLI-controlled JSON/error shapes. `auth` builds the OAuth client-credentials request, parses its token response, and controls one 401 recovery for cached tokens. `token_store` stores access tokens in a private Unix file or Windows Credential Manager. `http` configures the shared HTTP client and timeouts. `resources` contains endpoint-specific requests, response checks, and safe errors.

The [CLI foundation](cli-foundation.md) implements parsing, command discovery, and output behavior. `auth` uses an environment-provided Bearer token or reuses a cached client-credentials token until VUT returns 401; `http` executes read-only requests with fixed timeouts and no redirects. Resource modules implement [studies access](studies-access.md), [academic context reads](academic-context.md), [news and schedule](news-and-schedule.md), and fixed subject catalogue/Moodle-link/timetable and news-detail GETs for the [one-subject view](subject-view.md). `subject_view` resolves identity and composes sectioned CLI-owned JSON. `dates` validates CLI date inputs before credentials are read. The project is one Cargo package.

## Module ownership

| Module | Ownership |
| --- | --- |
| `cli` | Argument parsing, validation dispatch, exit behavior, and CLI-owned output envelopes |
| `auth` and `token_store` | OAuth credentials, one 401 recovery, and private token persistence |
| `http` | Shared bounded, no-redirect HTTP client configuration |
| `resources` | Endpoint-specific read requests, upstream response validation, and redacted errors |
| `subject_view` | Subject output model, source statuses, API section builders, fetch guard, composition, and the stable `fetch` entry point |
| `subject_view::lookup` | Active-study discovery, exact subject/offering selection, and default request scope |
| `subject_view::web` | Optional authenticated Studis and Moodle enrichment of an already composed view |
| `web_session` and `web` | Persistent browser session plus bounded parsers for fixed Studis and Moodle pages |

## Subject-view data flow

`cli` validates the selector and explicit date arguments before credentials are read. `subject_view::lookup` then reads either the selected study index or all active study indexes, resolves one offering, and derives the request window. The parent `subject_view` module fetches API sources under one guard and builds all six sections. `subject_view::web` may enrich those sections from the stored browser session. Finally, `cli` serializes the resulting schema-v1 `SubjectView`. Once identity is resolved, an individual source failure remains in its section as an explicit status and reason instead of discarding the other sections.

The study-index endpoint can take a caller-supplied numeric study ID. For `subjects show CODE_OR_NAME`, `subject_view` reads the studies list once, validates active markers, and reads every active study index before selecting anything. An explicit `--study-id` can address an inactive study. The selected index records are passed directly into composition, including distinct records for the same offering; there is no second index fetch. Account roles, news, and schedule endpoints use the authenticated person's context. Each endpoint needs its own documented behavior, offline request/response tests, and read-only live check. Token renewal means a fresh client-credentials grant; it is not a refresh-token flow. Default tests and CI never contact VUT.

On macOS, `token_store` puts its cache below Application Support. On Linux, it uses the XDG state directory. The module opens the private directory and its files without following final-component symlinks, validates object type, owner, and Unix mode through the opened descriptor, and performs cache operations relative to that directory descriptor. macOS validation also rejects extended ACLs through the open descriptor. It writes a same-directory mode-`0600` temporary file, syncs and validates it, atomically renames it over the cache, and syncs the directory. A separate stable mode-`0600` file provides a bounded cross-process lock. `auth` reads before locking for the normal hit path; a miss or cached-token 401 takes the lock, rereads, and grants only if no other process has saved a suitable token. The lock is released before the resource GET.

Windows retains Credential Manager because portable Rust file permissions do not establish a private Windows DACL. CI exercises the Windows backend with a uniquely named synthetic entry in separate processes and cleans it up. The Unix file cache deliberately does not inspect or migrate legacy Keychain or Secret Service entries, avoiding an authentication prompt during migration.

The individual authenticated commands return upstream-owned JSON under `raw`. `subjects show` returns CLI-owned sections with source URLs, evaluation times, statuses and reasons; nested source records retain upstream-owned fields. A failed source after identity resolution stays in an explicit unavailable section. API news details are fetched only after the other API sections, so a detail rate limit cannot suppress already collected catalogue, schedule, or Moodle-link results. The implemented VUT operations are reads; endpoint provenance and observed limits are documented with each command. `web_session` owns a persistent browser profile for user-driven sign-in and subsequent headless page reads. `web::studis` and `web::moodle` parse fixed authenticated pages into bounded data. Browser enrichment does not change the source API record types.
