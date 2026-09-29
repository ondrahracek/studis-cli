//! Argument parsing, command dispatch, and CLI-owned output contracts.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
};

use crate::resources::studies;

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
}

#[derive(Subcommand)]
enum StudiesCommand {
    /// Return studies from VUT as upstream-owned JSON.
    List,
}

#[derive(Serialize)]
struct Capabilities<'a> {
    schema_version: u8,
    cli_version: &'a str,
    commands: &'a [&'a str],
}

#[derive(Serialize)]
struct RawStudies<'a> {
    schema_version: u8,
    raw: &'a serde_json::Value,
}

fn write_json(output: &impl Serialize) -> ExitCode {
    let json = serde_json::to_string(output).expect("CLI output is serializable");
    match writeln!(io::stdout().lock(), "{json}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("studis: cannot write output: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parse arguments and run the selected command.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Capabilities => {
            let output = Capabilities {
                schema_version: 1,
                cli_version: env!("CARGO_PKG_VERSION"),
                commands: &["capabilities", "studies list"],
            };
            write_json(&output)
        }
        Command::Studies { command } => match command {
            StudiesCommand::List => match studies::fetch() {
                Ok(raw) => write_json(&RawStudies {
                    schema_version: 1,
                    raw: &raw,
                }),
                Err(message) => {
                    eprintln!("studis: {message}");
                    ExitCode::FAILURE
                }
            },
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
        let output = serde_json::to_value(RawStudies {
            schema_version: 1,
            raw: &raw,
        })
        .expect("serialize raw studies");

        assert_eq!(output, json!({"schema_version": 1, "raw": raw}));
    }
}
