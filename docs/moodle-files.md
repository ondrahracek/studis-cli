# Moodle resource files

`studis subjects files CODE_OR_NAME` lists direct Moodle `resource` activities for one resolved subject offering. It is a bounded, read-only operation: the command reads the documented VUT Moodle-link endpoint, at most one authenticated Studis catalogue page when that endpoint has no link, and one Moodle course overview. It does not open the listed activity URLs.

The subject selector is identical to `subjects show`: exact codes win over exact names, matching is case-insensitive, and ambiguous active studies or offerings require `--study-id` or `--offering-id`. Historical access can use both explicit IDs. Moodle course IDs are independent identifiers; the CLI accepts only a VUT-provided HTTPS `moodle.vut.cz/course/view.php?id=…` URL with one positive numeric `id`, canonicalizes that URL, and never derives it from the offering ID. A Studis fallback link must be explicitly labelled Moodle. Multiple identical canonical links are accepted. Conflicting API links return `ambiguous_moodle_links` without consulting Studis; conflicting Studis fallback links are also rejected.

```sh
studis subjects files IZP
studis subjects files IZP --study-id 7 --offering-id 42
```

The command writes one schema-v1 JSON object after subject identity is resolved:

```json
{
  "schema_version": 1,
  "subject": {
    "offering_id": 42,
    "subject_id": 142,
    "faculty_id": 13,
    "academic_year": 2026,
    "semester_type_id": 2,
    "study_id": 7
  },
  "status": "available",
  "course_url": "https://moodle.vut.cz/course/view.php?id=9",
  "files": [
    {
      "module_id": 5,
      "name": "Slides",
      "section_id": 10,
      "section_title": "Lectures",
      "activity_url": "https://moodle.vut.cz/mod/resource/view.php?id=5"
    }
  ],
  "limitations": ["direct_resource_activities_only"]
}
```

`available` means at least one valid direct resource was found. Malformed resource activities are omitted; when valid resources remain, `limitations` also contains `malformed_resource_activities_skipped`. A page containing only malformed resources is `unavailable` with `invalid_web_page`. `empty` means the parsed course overview had no activities to return. Other `unavailable` reasons are `auth_required`, `permission_denied`, `unsupported`, `ambiguous_moodle_links`, `no_verified_moodle_link`, `browser_unavailable`, `web_profile_unsafe`, `web_fetch_failed`, `unexpected_web_page`, `rate_limited`, `invalid_response`, or `fetch_failed`. `unsupported` means the page exposed activity types such as folders but no direct resources; it is deliberately distinct from an empty course. When the API provides no Moodle link, a missing or expired Studis session is reported before `no_verified_moodle_link` because the authenticated catalogue page could still contain the link.

The list is intentionally incomplete. It excludes folders, quizzes, forums, assignments, SCORM, H5P, embedded links, and resources hidden from the current user. Titles and section labels come from current Moodle HTML and can change independently of schema version; the surrounding object and field names are CLI owned.

## One-file download

Use a `module_id` returned by `subjects files` and choose a new local path:

```sh
studis subjects download IZP --file 5 --output ./slides.pdf
```

The command repeats the bounded listing and downloads only an exact direct `resource` match. It never treats an offering ID as a Moodle course or module ID. On success it writes one schema-v1 JSON line:

```json
{"schema_version":1,"status":"downloaded","module_id":5,"output":"./slides.pdf","bytes":12345}
```

The VUT operations are read-only. The command opens the saved Moodle profile headlessly, reads browser-context cookies into process memory, and discards partitioned cookies plus domains that cannot match `moodle.vut.cz`. The direct HTTP transfer cannot safely reproduce Chrome's top-level-site partition context. It creates a short-lived client with automatic redirects disabled. Each request must remain on HTTPS `moodle.vut.cz` with the default port; domain, path, and secure cookie attributes are reapplied for each request, including cookies scoped specifically to `/pluginfile.php/…`. Login redirects and Moodle login HTML return `auth_required`; a Moodle HTML page with an error marker returns `permission_denied`. Ordinary HTML resource files remain supported. Only a complete HTTP 200 response without `Content-Range` from a final `/pluginfile.php/` URL is accepted. Cookies, response bodies, and server filenames are never printed or persisted by the transfer.

Downloads are capped at 100 MiB and five redirects. A `text/html` resource has a lower 8 MiB cap: after streaming it to the private temporary file, the CLI reads the complete bounded file and checks Moodle login and error-page structure before publication. This prevents a late error marker from becoming a successful download while allowing ordinary HTML resources. Other content stays streaming and is never held as one in-memory body. The explicit output path must not exist, and its parent must already be a directory. Content streams to a private same-directory temporary file (mode `0600` on Unix), is synced, and is atomically published without replacing an existing path. Any handled fetch, size, classification, or publication failure removes the temporary file and exits 1 with empty stdout. Invalid or missing arguments exit 2 before authentication.

Immediate process termination cannot run Rust cleanup code. Ctrl-C, `SIGKILL`, a power loss, or a process crash can therefore leave a private hidden file named `.OUTPUT_NAME.studis-PID-COUNTER.tmp` beside the requested output. After confirming that the download process has stopped, inspect that output directory and remove the matching temporary file manually. The intended output name is never published from an incomplete transfer.

The cookie transport was verified read-only against one consented direct resource: a browser-context request returned HTTP 206 with `Content-Range`, and an in-process scoped-cookie request followed the same-host redirect and read exactly 16 bytes without saving content. This verifies the supported transport shape, not universal access to every Moodle course or resource.
