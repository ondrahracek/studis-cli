//! Argument parsing, command dispatch, and CLI-owned output contracts.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
};

use crate::{
    dates,
    resources::{news, schedule, studies},
};

#[derive(Parser)]
#[command(
    name = "studis",
    version,
    about = "Unofficial VUT information system CLI",
    subcommand_required = true,
    disable_help_subcommand = true,
    color = clap::ColorChoice::Never
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List commands available in this build.
    Capabilities,
    /// Read information about your studies.
    Studies {
        #[command(subcommand)]
        command: StudiesCommand,
    },
    /// Read study news.
    News {
        #[command(subcommand)]
        command: NewsCommand,
    },
    /// Read your personal teaching schedule.
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
}

#[derive(Subcommand)]
enum StudiesCommand {
    /// Return studies from VUT as upstream-owned JSON.
    List,
}

#[derive(Subcommand)]
enum NewsCommand {
    /// Return study news from a calendar date.
    List {
        #[arg(long, value_name = "YYYY-MM-DD", help = "Start date passed to VUT as datum_od", value_parser = dates::date)]
        since: String,
    },
}

#[derive(Subcommand)]
enum ScheduleCommand {
    /// Return personal teaching for a local date-time window.
    Teaching {
        #[arg(long, value_name = "YYYY-MM-DDTHH:MM", help = "Local start date and time", value_parser = dates::local_datetime)]
        from: String,
        #[arg(long, value_name = "YYYY-MM-DDTHH:MM", help = "Local end date and time", value_parser = dates::local_datetime)]
        to: String,
    },
    /// Return teaching weeks related to a calendar-date window.
    Weeks {
        #[arg(long, value_name = "YYYY-MM-DD", help = "Start date for related teaching weeks", value_parser = dates::date)]
        from: String,
        #[arg(long, value_name = "YYYY-MM-DD", help = "End date for related teaching weeks", value_parser = dates::date)]
        to: String,
    },
}

#[derive(Serialize)]
struct Capabilities<'a> {
    schema_version: u8,
    cli_version: &'a str,
    commands: &'a [&'a str],
}

#[derive(Serialize)]
struct RawResponse<'a> {
    schema_version: u8,
    raw: &'a serde_json::Value,
}

fn write_raw_to(
    result: Result<serde_json::Value, &'static str>,
    writer: &mut impl Write,
) -> ExitCode {
    match result {
        Ok(raw) => write_json_to(
            &RawResponse {
                schema_version: 1,
                raw: &raw,
            },
            writer,
        ),
        Err(message) => {
            eprintln!("studis: {message}");
            ExitCode::FAILURE
        }
    }
}

fn write_raw(result: Result<serde_json::Value, &'static str>) -> ExitCode {
    write_raw_to(result, &mut io::stdout().lock())
}

fn validate_range(from: &str, to: &str) -> Result<(), ExitCode> {
    dates::ordered(from, to).map_err(|message| {
        eprintln!("studis: {message}");
        ExitCode::from(2)
    })
}

fn write_json_to(output: &impl Serialize, writer: &mut impl Write) -> ExitCode {
    let json = serde_json::to_string(output).expect("CLI output is serializable");
    match writeln!(writer, "{json}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("studis: cannot write output: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write_json(output: &impl Serialize) -> ExitCode {
    write_json_to(output, &mut io::stdout().lock())
}

/// Parse arguments and run the selected command.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Capabilities => {
            let output = Capabilities {
                schema_version: 1,
                cli_version: env!("CARGO_PKG_VERSION"),
                commands: &[
                    "capabilities",
                    "studies list",
                    "news list",
                    "schedule teaching",
                    "schedule weeks",
                ],
            };
            write_json(&output)
        }
        Command::Studies { command } => match command {
            StudiesCommand::List => write_raw(studies::fetch()),
        },
        Command::News { command } => match command {
            NewsCommand::List { since } => write_raw(news::fetch(&since)),
        },
        Command::Schedule { command } => match command {
            ScheduleCommand::Teaching { from, to } => {
                if let Err(code) = validate_range(&from, &to) {
                    return code;
                }
                write_raw(schedule::fetch_teaching(&from, &to))
            }
            ScheduleCommand::Weeks { from, to } => {
                if let Err(code) = validate_range(&from, &to) {
                    return code;
                }
                write_raw(schedule::fetch_weeks(&from, &to))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_studies_wrapper_preserves_upstream_fields() {
        let raw = json!({
            "format": "json",
            "data": {"studia": [{"studium_id": 7, "future_field": "kept"}]},
            "extra": {"unknown": true}
        });
        let output = serde_json::to_value(RawResponse {
            schema_version: 1,
            raw: &raw,
        })
        .expect("serialize raw studies");

        assert_eq!(output, json!({"schema_version": 1, "raw": raw}));
    }

    #[test]
    fn successful_raw_result_writes_complete_wrapped_json_line() {
        let raw = json!({"format":"json","data":{"dokumenty":[{"unknown":true}]}});
        let mut bytes = Vec::new();
        write_raw_to(Ok(raw.clone()), &mut bytes);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).expect("output JSON"),
            json!({"schema_version":1,"raw":raw})
        );
        assert!(bytes.ends_with(b"\n"));
    }
}
