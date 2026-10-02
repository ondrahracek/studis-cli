use assert_cmd::Command;
use serde_json::json;
#[cfg(unix)]
use std::{os::fd::OwnedFd, os::unix::net::UnixStream, process::Stdio};

fn studis() -> Command {
    let mut command = Command::cargo_bin("studis").expect("binary is built for integration tests");
    command
        .env_remove("VUT_API_CLIENT_UID")
        .env_remove("VUT_API_CLIENT_SECRET")
        .env_remove("VUT_API_ACCESS_TOKEN");
    command
}

#[test]
fn help_succeeds_without_stderr() {
    let output = studis().arg("--help").output().expect("run studis");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("Usage: studis"));
    assert!(stdout.contains("capabilities"));
    assert!(stdout.contains("account"));
    assert!(stdout.contains("subjects"));
}

#[test]
fn subject_help_explains_explicit_scope_and_json() {
    let group = studis()
        .args(["subjects", "--help"])
        .output()
        .expect("subject help");
    assert!(group.status.success());
    assert!(String::from_utf8_lossy(&group.stdout).contains("show"));

    let show = studis()
        .args(["subjects", "show", "--help"])
        .output()
        .expect("show help");
    assert!(show.status.success());
    let text = String::from_utf8_lossy(&show.stdout);
    for flag in [
        "--offering-id",
        "--study-id",
        "--from",
        "--to",
        "--news-since",
        "--max-news",
    ] {
        assert!(text.contains(flag), "missing {flag}");
    }
    assert!(text.contains("JSON"));
    assert!(text.contains("CODE_OR_NAME"));
}

#[test]
fn subject_show_positional_lookup_and_offering_only_reach_authentication() {
    for args in [
        vec!["subjects", "show", "IZP"],
        vec!["subjects", "show", "--offering-id", "42"],
    ] {
        let output = studis().args(args).output().expect("run shorthand lookup");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[test]
fn subject_show_rejects_missing_or_partial_flag_only_identity() {
    for args in [
        vec!["subjects", "show"],
        vec!["subjects", "show", "--study-id", "7"],
    ] {
        let output = studis().args(args).output().expect("run incomplete lookup");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("CODE_OR_NAME") || stderr.contains("--offering-id"));
        assert!(!stderr.contains("credentials"));
    }
}

#[test]
fn subject_show_keeps_the_explicit_route_until_authentication() {
    let output = studis()
        .args([
            "subjects",
            "show",
            "--study-id",
            "7",
            "--offering-id",
            "42",
            "--from",
            "2026-09-01T00:00",
            "--to",
            "2027-08-31T23:59",
            "--news-since",
            "2026-09-01",
        ])
        .output()
        .expect("run explicit subject route");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("credentials"));
}

#[test]
fn subject_show_rejects_invalid_scope_before_credentials() {
    for args in [
        vec![
            "subjects",
            "show",
            "--offering-id",
            "0",
            "--study-id",
            "7",
            "--from",
            "2026-09-29T08:00",
            "--to",
            "2026-09-29T18:00",
            "--news-since",
            "2026-09-01",
        ],
        vec![
            "subjects",
            "show",
            "--offering-id",
            "5",
            "--study-id",
            "7",
            "--from",
            "2026-09-29T18:00",
            "--to",
            "2026-09-29T08:00",
            "--news-since",
            "2026-09-01",
        ],
        vec![
            "subjects",
            "show",
            "--offering-id",
            "5",
            "--study-id",
            "7",
            "--from",
            "2026-09-29T08:00",
            "--to",
            "2026-09-29T18:00",
            "--news-since",
            "2026-02-30",
        ],
        vec![
            "subjects",
            "show",
            "--offering-id",
            "5",
            "--study-id",
            "7",
            "--from",
            "2026-09-29T08:00",
            "--to",
            "2026-09-29T18:00",
            "--news-since",
            "2026-09-01",
            "--max-news",
            "0",
        ],
    ] {
        let output = studis()
            .args(args)
            .output()
            .expect("run invalid subject show");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[test]
fn web_login_help_and_missing_browser_have_noninteractive_contract() {
    let help = studis()
        .args(["auth", "web", "login", "--help"])
        .output()
        .expect("web login help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("login"));

    let output = studis()
        .env("STUDIS_BROWSER_PATH", "/no/such/browser")
        .args(["auth", "web", "login"])
        .output()
        .expect("web login without browser");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));

    let moodle = studis()
        .env("STUDIS_BROWSER_PATH", "/no/such/browser")
        .args(["auth", "web", "login", "--target", "moodle"])
        .output()
        .expect("Moodle login without browser");
    assert_eq!(moodle.status.code(), Some(1));
    assert!(moodle.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&moodle.stderr).contains("credentials"));
}

#[test]
fn subject_files_help_and_missing_credentials_have_stable_contract() {
    let help = studis()
        .args(["subjects", "files", "--help"])
        .output()
        .expect("subject files help");
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("CODE_OR_NAME"));
    assert!(text.contains("--study-id"));
    assert!(text.contains("--offering-id"));

    let output = studis()
        .args(["subjects", "files", "IZP"])
        .output()
        .expect("subject files without API credentials");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("credentials"));
}

#[test]
fn subject_download_help_and_validation_have_stable_contract() {
    let help = studis()
        .args(["subjects", "download", "--help"])
        .output()
        .expect("subject download help");
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for expected in [
        "CODE_OR_NAME",
        "--file",
        "--output",
        "--study-id",
        "--offering-id",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }

    for args in [
        vec![
            "subjects", "download", "IZP", "--file", "0", "--output", "file.pdf",
        ],
        vec!["subjects", "download", "IZP", "--file", "5"],
    ] {
        let output = studis()
            .args(args)
            .output()
            .expect("invalid download command");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }

    let directory = std::env::temp_dir().join(format!(
        "studis-cli-download-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let output_path = directory.join("resource.bin");
    let missing_credentials = studis()
        .args(["subjects", "download", "IZP", "--file", "5", "--output"])
        .arg(&output_path)
        .output()
        .expect("download without credentials");
    assert_eq!(missing_credentials.status.code(), Some(1));
    assert!(missing_credentials.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing_credentials.stderr).contains("credentials"));
    assert!(!output_path.exists());

    std::fs::write(&output_path, b"existing").unwrap();
    let existing_output = studis()
        .args(["subjects", "download", "IZP", "--file", "5", "--output"])
        .arg(&output_path)
        .output()
        .expect("download to existing output");
    assert_eq!(existing_output.status.code(), Some(1));
    assert!(existing_output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&existing_output.stderr);
    assert!(stderr.contains("already exists"));
    assert!(!stderr.contains("credentials"));
    assert_eq!(std::fs::read(&output_path).unwrap(), b"existing");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn subject_commands_share_selector_validation_before_authentication() {
    for args in [
        vec!["subjects", "show", ""],
        vec!["subjects", "files", ""],
        vec![
            "subjects",
            "download",
            "",
            "--file",
            "5",
            "--output",
            "resource.bin",
        ],
    ] {
        let output = studis().args(args).output().expect("run empty selector");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "studis: CODE_OR_NAME must not be empty\n"
        );
    }

    for args in [
        vec!["subjects", "show", "--study-id", "7"],
        vec!["subjects", "files", "--study-id", "7"],
        vec![
            "subjects",
            "download",
            "--study-id",
            "7",
            "--file",
            "5",
            "--output",
            "resource.bin",
        ],
    ] {
        let output = studis().args(args).output().expect("run partial selector");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "studis: provide CODE_OR_NAME or both --study-id and --offering-id\n"
        );
    }
}

#[test]
fn academic_context_subcommand_help_lists_new_reads() {
    for (group, expected) in [
        ("studies", "index"),
        ("account", "roles"),
        ("schedule", "terms"),
    ] {
        let output = studis()
            .args([group, "--help"])
            .output()
            .expect("run subcommand help");

        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
    }
}

#[test]
fn version_reports_package_version() {
    let output = studis().arg("--version").output().expect("run studis");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 version"),
        format!("studis {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn capabilities_has_exact_versioned_json_contract() {
    let output = studis().arg("capabilities").output().expect("run studis");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(
        body,
        json!({
            "schema_version": 1,
            "cli_version": env!("CARGO_PKG_VERSION"),
            "commands": [
                "capabilities",
                "studies list",
                "studies index",
                "account roles",
                "news list",
                "schedule teaching",
                "schedule weeks",
                "schedule terms",
                "subjects show",
                "subjects files",
                "subjects download",
                "auth web login"
            ]
        })
    );

    let with_credentials = studis()
        .env("VUT_API_CLIENT_UID", "review-uid-sentinel")
        .env("VUT_API_CLIENT_SECRET", "review-secret-sentinel")
        .arg("capabilities")
        .output()
        .expect("run studis with placeholder credentials");
    assert!(with_credentials.status.success());
    assert!(with_credentials.stderr.is_empty());
    assert_eq!(with_credentials.stdout, output.stdout);
}

#[test]
fn unknown_command_exits_two_without_stdout() {
    let output = studis().arg("missing").output().expect("run studis");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
}

#[test]
fn capability_list_does_not_omit_an_implicit_help_subcommand() {
    let output = studis().arg("help").output().expect("run studis");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("help"));
}

#[test]
fn missing_command_exits_two_without_stdout() {
    let output = studis().output().expect("run studis");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn studies_list_rejects_missing_or_empty_credentials() {
    for (uid, secret) in [
        (None, None),
        (Some("dummy-uid"), None),
        (None, Some("dummy-secret")),
        (Some(""), Some("dummy-secret")),
        (Some("dummy-uid"), Some("")),
    ] {
        let mut command = studis();
        command.args(["studies", "list"]);
        if let Some(uid) = uid {
            command.env("VUT_API_CLIENT_UID", uid);
        }
        if let Some(secret) = secret {
            command.env("VUT_API_CLIENT_SECRET", secret);
        }

        let output = command.output().expect("run studies list");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
        assert!(stderr.contains("credentials"));
        assert!(!stderr.contains("dummy-uid"));
        assert!(!stderr.contains("dummy-secret"));
    }
}

#[test]
fn new_read_commands_reject_missing_credentials_without_network() {
    for args in [
        vec!["studies", "index", "--study-id", "7"],
        vec!["account", "roles"],
        vec!["schedule", "terms"],
        vec!["news", "list", "--since", "2026-09-29"],
        vec![
            "schedule",
            "teaching",
            "--from",
            "2026-09-29T08:00",
            "--to",
            "2026-09-29T18:00",
        ],
        vec![
            "schedule",
            "weeks",
            "--from",
            "2026-09-28",
            "--to",
            "2026-10-04",
        ],
    ] {
        let output = studis().args(args).output().expect("run read command");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[test]
fn schedule_terms_has_no_undocumented_date_flags() {
    for flag in ["--from", "--to"] {
        let output = studis()
            .args(["schedule", "terms", flag, "2026-09-29"])
            .output()
            .expect("run terms with unsupported flag");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[test]
fn invalid_study_ids_exit_two_before_credentials() {
    for args in [
        vec!["studies", "index"],
        vec!["studies", "index", "--study-id", "-1"],
        vec!["studies", "index", "--study-id", "not-a-number"],
        vec!["studies", "index", "--study-id", "18446744073709551616"],
    ] {
        let output = studis().args(args).output().expect("run invalid study ID");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[test]
fn empty_supplied_token_fails_without_using_client_credentials() {
    let output = studis()
        .env("VUT_API_ACCESS_TOKEN", "")
        .env("VUT_API_CLIENT_UID", "dummy-uid")
        .env("VUT_API_CLIENT_SECRET", "dummy-secret")
        .args(["studies", "list"])
        .output()
        .expect("run with empty token");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(stderr.contains("access token is empty"));
    assert!(!stderr.contains("dummy-uid"));
    assert!(!stderr.contains("dummy-secret"));
}

#[test]
fn invalid_dates_and_reversed_ranges_exit_two_before_credentials() {
    for args in [
        vec!["news", "list", "--since", "2026-02-30"],
        vec!["news", "list"],
        vec![
            "schedule",
            "teaching",
            "--from",
            "2026-09-29",
            "--to",
            "2026-09-30",
        ],
        vec![
            "schedule",
            "teaching",
            "--from",
            "2026-09-29T08:00",
            "--to",
            "2026-09-29T24:00",
        ],
        vec![
            "schedule",
            "teaching",
            "--from",
            "2026-09-29T18:00",
            "--to",
            "2026-09-29T08:00",
        ],
        vec![
            "schedule",
            "weeks",
            "--from",
            "2026-10-04",
            "--to",
            "2026-09-28",
        ],
        vec!["schedule", "weeks", "--from", "2026-09-28"],
        vec![
            "schedule",
            "weeks",
            "--from",
            "2026-02-30",
            "--to",
            "2026-03-01",
        ],
        vec![
            "schedule",
            "weeks",
            "--from",
            "2026-03-01",
            "--to",
            "2026-02-30",
        ],
    ] {
        let output = studis()
            .args(args)
            .output()
            .expect("run invalid read command");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("credentials"));
    }
}

#[cfg(unix)]
#[test]
fn closed_output_pipe_does_not_panic() {
    let (writer, reader) = UnixStream::pair().expect("create pipe");
    drop(reader);
    let stdout = Stdio::from(OwnedFd::from(writer));

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("studis"))
        .arg("capabilities")
        .stdout(stdout)
        .output()
        .expect("run studis with closed output pipe");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}
