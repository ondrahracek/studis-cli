use assert_cmd::Command;
use serde_json::json;
#[cfg(unix)]
use std::{os::fd::OwnedFd, os::unix::net::UnixStream, process::Stdio};

fn studis() -> Command {
    let mut command = Command::cargo_bin("studis").expect("binary is built for integration tests");
    command
        .env_remove("VUT_API_CLIENT_UID")
        .env_remove("VUT_API_CLIENT_SECRET");
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
            "commands": ["capabilities"]
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
