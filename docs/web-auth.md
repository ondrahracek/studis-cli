# Authenticated web session

`studis auth web login` opens a headed Chrome or Chromium window for VUT Single Sign-On. Sign in in that window, including any verification step. The command waits up to five minutes and reports success only after Studis is visible. It never accepts a password as a CLI argument or reads one from `.env`.

```sh
studis auth web login
studis subjects show --offering-id 42 --study-id 7 \
  --from 2026-09-28T00:00 --to 2026-10-05T23:59 \
  --news-since 2026-09-01
```

The second command opens the saved profile headlessly and reads only fixed Studis pages and the VUT-provided Moodle course URL. It still needs separate VUT API credentials for the API-backed sections. If the web login has expired, web data stays unavailable with `auth_required`; run `studis auth web login` again yourself. Agents can use the saved session without a password prompt while VUT accepts it. Browser and API authentication are separate.

The browser profile is below the platform's local application data directory under `studis-cli/web-profile` (on Linux, the XDG state directory when provided). Set `STUDIS_WEB_PROFILE_DIR` to an absolute path to choose another profile; on Unix that directory must belong to the current user and have mode `0700`, and every existing ancestor must be a real directory owned by the user or root without group or other write access. On macOS, ancestors with an ACL allow entry are rejected; the standard deny-delete ACL on a home directory is accepted. Set `STUDIS_BROWSER_PATH` to a Chrome/Chromium executable if automatic discovery fails. Chrome or Chromium must already be installed; the CLI does not install it. The profile contains authentication cookies and browsing state. Treat it as a credential: do not share, commit, back up to a public location, or expose it to other users. On Windows, keep a custom profile path in a user-private location. A concurrent Chrome process using the same profile may prevent a second launch.

While Chrome runs, its DevTools control port listens on localhost. Another user or process on the same host may be able to attach and read the signed-in browser session. Use this command only on a trusted local machine, and close the CLI when finished. The CLI checks TLS certificates and validates the final HTTPS host, path, and expected query parameters before parsing a page. A redirect to sign-in is reported as an authentication failure. It limits each HTML document to 2 MiB and only extracts visible text, links, and page structure needed for the subject view; it does not submit forms. Studis and Moodle HTML can change without notice, so check each section's `status`, `reason`, and `limitations` before relying on it. Routine tests use synthetic HTML and never contact VUT.
