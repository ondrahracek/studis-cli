# One-subject view

`studis subjects show` composes read-only VUT API and optional authenticated Studis/Moodle page data for one term-specific subject offering. It writes one CLI-owned JSON object to stdout. It does not sign up for anything, change registrations, or submit work. Optional browser enrichment uses a persistent local Chrome profile, which Chrome may update during a read.

```sh
studis subjects show IZP

# Existing fully explicit route for stable scripts:
studis studies list
studis subjects show \
  --offering-id 42 \
  --study-id 7 \
  --from 2026-09-28T00:00 \
  --to 2026-10-05T23:59 \
  --news-since 2026-09-01 \
  --max-news 10
```

`CODE_OR_NAME` first matches `zkratka` exactly and case-insensitively. Only when no code matches does it match `ap_nazev` the same way. Without `--study-id`, the CLI fetches the studies list once, requires every `aktivni_studium` marker to be numeric `0` or `1`, and fetches every active study index. It never selects from a partial subset after an index failure. Matches in multiple active studies fail with candidate IDs and `--study-id` guidance. No active match asks for an explicit study so historical records are not selected silently. An explicit study may be inactive.

Within one study, all rows for a selected offering are retained, including distinct `el_index_id` records. When a code or name matches multiple offerings, the unique offering whose `zacatek_semestru`–`konec_semestru` interval contains the current date in the `Europe/Prague` time zone wins. Otherwise the latest semester start wins. Current-overlap ambiguity, a latest-start tie, or conflicting subject identity fails with candidate IDs and `--offering-id` guidance. `--offering-id` may also be used alone to infer its active study. A code/name combined with IDs must match them before composition starts.

For shorthand lookup, the default teaching window covers the selected academic year from 1 September at `00:00` through 31 August at `23:59`, derived from `zacatek_semestru`; `--from` and `--to` independently override those bounds. News defaults to `1900-01-01`. With no `--max-news`, every matching row returned by the news-list response is selected; `requested.max_news` is the numeric sentinel `0`. A caller cap accepts 1–50. The fully explicit flag-only route still requires both IDs and all three date inputs and defaults `--max-news` to 10.

`--study-id` is the `studium_id` returned by `studies list`. `--offering-id` is `aktualni_predmet_id`, called `apid` in Studis URLs. It identifies a particular academic-year offering; `predmet_id` is the permanent subject ID and is **not** interchangeable. Moodle's numeric course ID must not be guessed from either ID. The command reads the Moodle link returned by VUT instead. Teaching bounds are local `YYYY-MM-DDTHH:MM` values; the news-since date uses `YYYY-MM-DD`.

The command first resolves the offering in the selected study index. An absent offering, inconsistent identity, invalid index, or authentication failure exits 1 with empty stdout. Invalid argument syntax and two explicit reversed teaching bounds exit 2 before authentication. When one bound is omitted, a reversed effective window is detected after the offering is resolved and exits 2. Once identity is resolved, independent sources may fail: the command still exits 0 and reports that section as `unavailable`. Agents must check each section's `status` and `reason` before using its `data`.

## Output

This synthetic example shows the shape; its IDs and content do not describe a real student:

```json
{
  "schema_version": 1,
  "subject": {"offering_id": 42, "subject_id": 9, "faculty_id": 13, "academic_year": 2026, "semester_type_id": 2, "study_id": 7},
  "requested": {"from": "2026-09-28T00:00", "to": "2026-10-05T23:59", "news_since": "2026-09-01", "max_news": 10},
  "sections": {
    "catalog": {"status": "available", "sources": [{"url": "https://api.vut.cz/api/predmety/aktualni_predmet/42/v1", "fetched_at_unix_ms": 0}], "data": {"records": [{"predmet_id": 9, "fakulta_id": 13, "aktualni_predmety": [{"aktualni_predmet_id": 42}]}], "web_detail_url": "https://www.vut.cz/studis/student.phtml?gm=gm_detail_predmetu&apid=42"}, "limitations": ["rich_catalogue_text_requires_web_access"]},
    "study_record": {"status": "available", "sources": [{"url": "https://api.vut.cz/api/moje/studia/studium/7/index/v1", "fetched_at_unix_ms": 0}], "data": {"index_entries": [{"aktualni_predmet_id": 42, "predmet_id": 9}]}, "limitations": ["assessment_table_may_require_web_access"]},
    "announcements": {"status": "empty", "sources": [{"url": "https://api.vut.cz/api/moje/studia/aktuality/v1?datum_od=2026-09-01", "fetched_at_unix_ms": 0}], "data": {"items": [], "returned_count": 0, "matching_count": 0, "truncated": false, "complete": true}, "limitations": ["news_api_may_omit_web_body_links"]},
    "personal_schedule": {"status": "empty", "sources": [{"url": "https://api.vut.cz/api/rozvrh/osobni/vyucovani/v4?datum_od=2026-09-28T00%3A00&datum_do=2026-10-05T23%3A59", "fetched_at_unix_ms": 0}], "data": {"entries": []}},
    "course_timetable": {"status": "unavailable", "reason": "unverified_empty_response", "sources": [{"url": "https://api.vut.cz/api/rozvrh/aktualni_predmet/42/vyucovani/v1?datum_od=2026-09-28&datum_do=2026-10-05", "fetched_at_unix_ms": 0}], "data": null},
    "moodle": {"status": "unavailable", "reason": "auth_required", "sources": [{"url": "https://api.vut.cz/api/predmety/aktualni_predmet/42/odkazy/moodle/v1", "fetched_at_unix_ms": 0}], "data": {"course_url": "https://moodle.vut.cz/course/view.php?id=42", "sections": []}}
  },
  "warnings": []
}
```

`sources` records source API URLs and the time each section or detail was evaluated in Unix milliseconds. A source reference can name a request skipped after authentication rejection or rate limiting; it does not prove a fetch succeeded or that the response was complete. `available` means this CLI could parse usable source data; `empty` means the supported source returned no matching records in this response, including VUT's observed `data:{}` empty form for personal teaching or news. It does not prove there are no real-world classes or announcements outside the request's scope. `unavailable` means the CLI cannot establish that section's data and supplies a machine-readable `reason`.

An announcement has its own `detail_status` and `reason` when its list row was available but the single-item GET failed or was skipped. `matching_count` counts rows returned by the news-list call that matched both IDs. `truncated` is true only when a caller's `--max-news` cap omitted matching returned rows. `complete` is true only when every matching row in this one API response was selected and its detail hydrated. It never claims the list contains all historical news. The CLI stops starting new API detail requests after 60 seconds; a detail operation started just before that point can run longer while its GET, possible token renewal, and retry finish. The 60 seconds is a start deadline, not a hard runtime limit. Remaining rows become `budget_exhausted`. A 429 stops further detail requests and marks remaining rows `rate_limited`. Catalogue, schedules, and the Moodle link are fetched before news details, so those sections remain available when detail hydration becomes partial.

The objects in `catalog.data.records`, `study_record.data.index_entries`, announcement `summary`/`detail`, and schedule `entries` retain VUT's fields. Those nested fields are upstream-owned and may change. The surrounding section names, statuses, reasons, provenance structure, and count fields are CLI-owned under `schema_version:1`. This output can contain personal study information. Keep it out of public logs, issue attachments, and committed fixtures.

## Sources and limits

| Section | Documented read-only VUT GET | Meaning and limit |
| --- | --- | --- |
| `study_record` | [Study index](https://api.vut.cz/doc/area/1789/endpoint/417368/method/4) | Retains every matching row from the selected study; carries personal result fields, but a full assessment table is not established. |
| `catalog` | [Subject catalogue](https://api.vut.cz/doc/area/1787/endpoint/417354/method/4) | Core metadata for the offering; rich Studis prose, syllabus, and literature are not returned by the observed API response. |
| `announcements` | [Study news list](https://api.vut.cz/doc/area/1789/endpoint/417374/method/4) and [single news](https://api.vut.cz/doc/area/1789/endpoint/417375/method/4) | Filters by study and offering, sorts the returned rows by publication timestamp, then fetches all returned matches or the caller's `--max-news` cap while starting no new API detail GET after a 60-second deadline; an in-flight detail operation may exceed that deadline, including token renewal and a retry. Skipped details stay explicit. The API may omit links present in the Studis page. |
| `personal_schedule` | [Personal teaching](https://api.vut.cz/doc/area/1793/endpoint/422106/method/4) | Filters returned teaching by offering ID. This is the student's view; VUT's exact boundary and pagination semantics remain upstream-defined. |
| `course_timetable` | [Offering teaching](https://api.vut.cz/doc/area/1793/endpoint/417397/method/4) | Uses the calendar dates touched by the teaching window. It may include other groups. An observed `data:{}` is reported as `unavailable`, since the Studis calendar can still show classes. |
| `moodle` | [VUT Moodle link](https://api.vut.cz/doc/area/1787/endpoint/417359/method/4) | Accepts only an HTTPS `moodle.vut.cz` link. Repeated identical canonical links are accepted; distinct valid links report `ambiguous_moodle_links`. A reusable authenticated browser session reads the linked course's visible sections and activities. Missing login is reported as unavailable. |

After explicit web login, the CLI extracts rich catalogue fields, personal subject fields and assessments, announcement bodies and links, calendar events, and Moodle sections and activities from read-only pages. Studis and Moodle reads are independent: expired Studis authentication does not suppress a Moodle course read from the same saved profile. Web enrichment is best effort: per-section statuses and limitations describe missing or rejected pages. The web timetable is used when the API gives an unverified empty response; it is only marked available or empty when the page covers the requested calendar dates and event times can be checked against the requested hour bounds. Course-wide events may include other groups. A missing or rejected browser session is never reported as an empty Moodle course. See [web login](web-auth.md) for setup and session security. HTTP 403 and 429 are distinct `permission_denied` and `rate_limited` section reasons when they occur after identity resolution. After a final API authentication rejection or rate limit, the command skips later source GETs and reports their sections as unavailable. No routine test or CI job contacts VUT; synthetic fixtures test requests, parsers, joins, and output behavior.

An agent can run the command, parse stdout as JSON only after exit 0, and branch on `sections.<name>.status`. For example, treat `sections.moodle.data.course_url` as a link when its `reason` is `auth_required`; do not treat its empty `sections` array as proof that the Moodle course has no activities.
