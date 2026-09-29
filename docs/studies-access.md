# Read-only studies access

`studis studies list` uses an environment-provided Bearer token or obtains a VUT OAuth client-credentials token, then reads `GET /api/moje/studia/v1`. It performs no VUT data write action. The command is an unofficial student-maintained integration, not a VUT service.

## Endpoint provenance and observed limits

- [VUT's personal API access guide](https://doc.vut.cz/cs/UzivatelskeUctyPrukazyIdentita/PristupAPI) describes the user-owned client credentials. A read-only probe verified HTTP Basic client authentication with `grant_type=client_credentials` at `https://id.vut.cz/auth/common/oauth2/token`; the returned token was Bearer. [OAuth client credentials](https://www.rfc-editor.org/rfc/rfc6749.html#section-4.4) defines the grant used here.
- The authenticated [legacy studies endpoint documentation](https://api.vut.cz/doc/area/1789/endpoint/417366/method/4) lists `GET /api/moje/studia/v1`. Read-only checks with one client returned HTTP 200 and a JSON object with top-level `format` and `data`, with `data.studia` an array. No pagination-looking header or top-level/data key appeared in the observed response. This does not prove every account has the same permissions or that pagination never exists.
- The CLI reads `VUT_API_CLIENT_UID` and `VUT_API_CLIENT_SECRET` from exported environment variables, or uses a nonempty `VUT_API_ACCESS_TOKEN` when supplied. The supplied token takes precedence, bypasses the CLI-managed token cache, and is not renewed. With client credentials, an access token is cached by UID in a private Unix file or Windows Credential Manager and reused until a GET returns 401. The Unix cache is plaintext with owner-only directory and file controls; system administrators can still read it. Only a cached token's 401 starts one replacement client-credentials grant and one GET retry. It does not read `.env` itself, accept credentials as arguments, parse token expiry, retry other failures, or allow an alternate endpoint URL. The HTTP client has a 10-second connect timeout, a 20-second total timeout per request, and does not follow redirects.

## Output contract

Success writes one newline-terminated JSON object:

```json
{"schema_version":1,"raw":{"data":{"studia":[]}}}
```

The example is synthetic. Only the placement of `schema_version` and `raw` belongs to the CLI's stable JSON contract. `raw` contains the complete VUT response as a JSON value; its keys and values are upstream-owned and may change without a CLI schema-version bump. Whitespace, object key order, and number spelling from the HTTP body are not preserved. The CLI accepts an empty `data.studia` array and rejects a missing or non-array value instead of silently reporting an empty list.

Operational failures exit 1, leave stdout empty, and print a short diagnostic on stderr. Clap argument errors retain exit 2. Diagnostics do not include credentials, token, Basic header, or upstream error body. A closed stdout pipe exits without a panic. The command is listed as `"studies list"` in `studis capabilities`.

## Verification approach

Routine tests and CI stay offline. Process tests cover command discovery and missing, empty, or partial credential variables. Unit tests use synthetic token/studies JSON and inspect constructed requests with dummy credentials without sending them. A separate read-only live CLI check piped the personal JSON into a metadata-only consumer; it confirmed the wrapper, newline, and `data.studia` array without printing or saving study values. That check establishes interoperability only for the tested client at the time of the check.

## Scope decisions

The user explicitly rejected local mock servers and recreating VUT's API. This implementation uses pure request/response and token-state tests plus separate real read-only API checks. It keeps complete upstream JSON under `raw` because field semantics have not been verified enough for a stable curated study model. The command has no endpoint override or VUT data write operation. The cache uses a prompt-free private file in macOS Application Support or Linux XDG state, and Windows Credential Manager on Windows. Unsafe or unavailable storage fails without a less-restricted fallback. Client-credentials grants do not use OAuth refresh tokens.
