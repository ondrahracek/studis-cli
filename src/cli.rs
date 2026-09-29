//! Argument parsing, command dispatch, and CLI-owned output contracts.

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
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
}

#[derive(Serialize)]
struct Capabilities<'a> {
    schema_version: u8,
    cli_version: &'a str,
    commands: &'a [&'a str],
}

/// Parse arguments and run the selected command.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Capabilities => {
            let output = Capabilities {
                schema_version: 1,
                cli_version: env!("CARGO_PKG_VERSION"),
                commands: &["capabilities"],
            };
            let json =
                serde_json::to_string(&output).expect("static capabilities serialize to JSON");
            match writeln!(io::stdout().lock(), "{json}") {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("studis: cannot write output: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
