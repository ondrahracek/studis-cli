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
                "news list",
                "schedule teaching",
                "schedule weeks"
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
