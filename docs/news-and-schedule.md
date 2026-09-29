# Study news and personal schedule

These commands make one documented VUT GET. They use an already-issued Bearer token from `VUT_API_ACCESS_TOKEN` when present, or obtain one using `VUT_API_CLIENT_UID` and `VUT_API_CLIENT_SECRET`. The token source is the process environment; the CLI does not read `.env` itself. They do not select or save a study ID, persist a token, follow redirects, retry, or perform a VUT data write. Authentication setup and the JSON ownership boundary are also described in [studies access](studies-access.md).

| Command | Documented GET | Query mapping | Expected data |
| --- | --- | --- | --- |
| `studis news list --since YYYY-MM-DD` | [`/api/moje/studia/aktuality/v1`](https://api.vut.cz/doc/area/1789/endpoint/417374/method/4) | `datum_od` | `data.dokumenty` |
| `studis schedule teaching --from YYYY-MM-DDTHH:MM --to YYYY-MM-DDTHH:MM` | [`/api/rozvrh/osobni/vyucovani/v4`](https://api.vut.cz/doc/area/1793/endpoint/422106/method/4) | `datum_od`, `datum_do` | `data.vyucovani` |
| `studis schedule weeks --from YYYY-MM-DD --to YYYY-MM-DD` | [`/api/rozvrh/osobni/vyucovani/tydny/v2`](https://api.vut.cz/doc/area/1793/endpoint/420202/method/4) | `datum_od`, `datum_do` | `data.tydny` |

The authenticated documentation calls teaching bounds “date and time” and presents local date-time inputs; the CLI passes that local form through without timezone conversion. The API's interpretation of daylight-saving transitions and inclusivity of bounds remains unverified. Weeks uses date inputs, but a narrow window may return all weeks in the related term: two different one-week windows in the same term produced identical payloads for one account. Treat `weeks` as related-week data, not exact events within the requested dates. `rows_count` in that response did not equal the number of `tydny` elements in the observed response; the CLI preserves both values and does not infer pagination.

Success writes one newline-terminated JSON object: `{"schema_version":1,"raw":<complete VUT response>}`. Fields under `raw` are upstream-owned. For a window with no data, VUT was observed returning `"data":{}` rather than an empty array; this shape is accepted and preserved. A nonempty `data` object must contain the route's expected array. Malformed JSON or a different nonempty data shape fails with exit 1 and empty stdout. Invalid calendar dates, local times, or reversed ranges fail with exit 2 before authentication. Diagnostics are short and do not include the token, credentials, URL query values, or upstream body.

The read-only research checks observed HTTP 200 and the expected array keys for one client on 29 September 2026. Other users may have different permissions or data. These checks do not establish complete history or universal pagination behavior. Routine tests use synthetic JSON and inspect built requests without contacting VUT; live checks remain opt-in and read-only.
