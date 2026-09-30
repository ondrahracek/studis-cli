# Academic context reads

These commands read academic context through documented VUT GET endpoints. They perform no VUT data write action.

| Command | Documented GET | Input | Required response envelope |
| --- | --- | --- | --- |
| `studis studies index --study-id ID` | [`/api/moje/studia/studium/{studium_id}/index/v1`](https://api.vut.cz/doc/area/1789/endpoint/417368/method/4) | Numeric `studium_id` from `studis studies list` | `data.studia` array or observed empty `data:{}` |
| `studis account roles` | [`/api/moje/info/role/v1`](https://api.vut.cz/doc/area/1790/endpoint/417438/method/4) | Authenticated account | `data.role` array |
| `studis schedule terms` | [`/api/rozvrh/osobni/terminy/v3`](https://api.vut.cz/doc/area/1793/endpoint/422093/method/4) | Authenticated account; no query flags | `data.terminy` array |

The index command requires the study ID explicitly. It does not choose, infer, or store a current study. VUT documents the parameter only as a number, so the CLI accepts unsigned integers without inventing a positivity or narrower range rule. Missing, negative, nonnumeric, or values above the `u64` range fail with exit 2 before credentials are read. The value becomes one fixed path segment; the request has no query or body. The roles request also has no query or body.

The terms method lists language, date, subject, year, semester-type, and room query parameters without required markers. The CLI supplies none of them because their default range and completeness semantics have not been established. `schedule terms` returns the terms selected by VUT for the unparameterized request. It has no `--from` or `--to` flags and does not promise all terms, complete history, or an exact date window.

Success writes one newline-terminated JSON object: `{"schema_version":1,"raw":<complete VUT response>}`. Fields under `raw` are upstream-owned and may change independently of the CLI schema version. The index endpoint accepts an observed empty `data:{}` response as well as a `data.studia` array. Roles and terms still require their endpoint-specific arrays. Malformed JSON, absent/null `data`, or a nonempty object without the required array fails with exit 1 and empty stdout.

All three commands use `VUT_API_ACCESS_TOKEN` when supplied or the existing client-credentials and token-cache flow. A cached token rejected with HTTP 401 causes one new client-credentials grant and one retry. HTTP 403 remains a distinct redacted access-denied error. Other operational failures also exit 1 without printing credentials, tokens, request inputs, or upstream response bodies.

A metadata-only read with one client on 29–30 September 2026 found integer study IDs in `studies list`, HTTP 200 from all three endpoints, `data.studia` as an array whose inspected index item had `index` and `studium_id` keys, an empty `data:{}` index for another listed study, `data.role` as an array, and `data.terminy` as an array. No account values were retained. The empty-object form is observed behavior, not a documented promise. These checks do not establish access for every OAuth client, pagination behavior, or completeness across all VUT systems. Account roles describe returned account context; they do not guarantee permission to every API route.

Routine tests stay offline. They validate argument handling before authentication, fixed GET construction with dummy credentials, strict endpoint-specific envelopes, preservation of unknown fields, command discovery, and missing-credential behavior. Live interoperability checks remain explicit, read-only, and metadata-only.
