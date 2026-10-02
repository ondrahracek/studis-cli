//! Argument parsing, command dispatch, and CLI-owned output contracts.

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::{
    io::{self, Write},
    num::NonZeroU64,
    path::PathBuf,
    process::ExitCode,
};

use crate::{
    dates, moodle_download, moodle_files,
    resources::{account, news, schedule, studies},
    subject_view,
    web_session::{LoginTarget, WebSession},
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
    /// Read account context.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
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
    /// Compose read-only information about one subject offering.
    Subjects {
        #[command(subcommand)]
        command: SubjectsCommand,
    },
    /// Manage a separate user-driven web login for Studis and Moodle reads.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Manage the isolated browser session.
    Web {
        #[command(subcommand)]
        command: WebAuthCommand,
    },
}

#[derive(Subcommand)]
enum WebAuthCommand {
    /// Open a headed browser for VUT SSO; no password enters the CLI.
    Login {
        #[arg(long, value_enum, default_value_t)]
        target: LoginTarget,
    },
}

#[derive(Args)]
struct SubjectSelector {
    #[arg(
        value_name = "CODE_OR_NAME",
        help = "Exact subject code, or exact display name if no code matches"
    )]
    code_or_name: Option<String>,
    #[arg(
        long,
        value_name = "ID",
        help = "Optional term-specific offering selector (Studis apid)"
    )]
    offering_id: Option<NonZeroU64>,
    #[arg(
        long,
        value_name = "ID",
        help = "Optional study selector from studies list"
    )]
    study_id: Option<NonZeroU64>,
}

impl SubjectSelector {
    fn validate(&self) -> Result<(), ExitCode> {
        if self.code_or_name.is_none() && self.offering_id.is_none() {
            eprintln!("studis: provide CODE_OR_NAME or both --study-id and --offering-id");
            return Err(ExitCode::from(2));
        }
        if self
            .code_or_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            eprintln!("studis: CODE_OR_NAME must not be empty");
            return Err(ExitCode::from(2));
        }
        Ok(())
    }

    fn is_explicit_route(&self) -> bool {
        self.code_or_name.is_none() && self.study_id.is_some() && self.offering_id.is_some()
    }

    fn into_request(self) -> subject_view::SubjectRequest {
        subject_view::SubjectRequest {
            code_or_name: self.code_or_name,
            offering_id: self.offering_id.map(NonZeroU64::get),
            study_id: self.study_id.map(NonZeroU64::get),
            from: None,
            to: None,
            news_since: None,
            max_news: 0,
        }
    }
}

#[derive(Subcommand)]
enum SubjectsCommand {
    /// Return one subject view as CLI-owned JSON; sections report unavailable sources.
    Show {
        #[command(flatten)]
        selector: SubjectSelector,
        #[arg(long, value_name = "YYYY-MM-DDTHH:MM", help = "Local teaching-window start", value_parser = dates::local_datetime)]
        from: Option<String>,
        #[arg(long, value_name = "YYYY-MM-DDTHH:MM", help = "Local teaching-window end", value_parser = dates::local_datetime)]
        to: Option<String>,
        #[arg(long, value_name = "YYYY-MM-DD", help = "Start date for subject announcements", value_parser = dates::date)]
        news_since: Option<String>,
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(1..=50), help = "Cap matching announcements to hydrate (1–50); shorthand defaults to all returned rows")]
        max_news: Option<u8>,
    },
    /// List direct Moodle resource activities for one subject as CLI-owned JSON.
    Files {
        #[command(flatten)]
        selector: SubjectSelector,
    },
    /// Download one listed Moodle resource without overwriting a file.
    Download {
        #[arg(
            long,
            value_name = "ID",
            help = "Moodle module ID returned by subjects files"
        )]
        file: NonZeroU64,
        #[arg(long, value_name = "PATH", help = "New output path (must not exist)")]
        output: PathBuf,
        #[command(flatten)]
        selector: SubjectSelector,
    },
}

#[derive(Subcommand)]
enum StudiesCommand {
    /// Return studies from VUT as upstream-owned JSON.
    List,
    /// Return the index for an explicit study as upstream-owned JSON.
    Index {
        #[arg(long, value_name = "ID", help = "Numeric study ID from studies list")]
        study_id: u64,
    },
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Return account roles from VUT as upstream-owned JSON.
    Roles,
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
    /// Return the terms selected by VUT for your account.
    Terms,
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

fn default_news_limit(explicit_route: bool, requested: Option<u8>) -> usize {
    requested
        .map(usize::from)
        .unwrap_or(if explicit_route { 10 } else { 0 })
}

fn subject_error_exit_code(message: &str) -> ExitCode {
    if message == "--from must not be after --to" {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
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
                    "studies index",
                    "account roles",
                    "news list",
                    "schedule teaching",
                    "schedule weeks",
                    "schedule terms",
                    "subjects show",
                    "subjects files",
                    "subjects download",
                    "auth web login",
                ],
            };
            write_json(&output)
        }
        Command::Studies { command } => match command {
            StudiesCommand::List => write_raw(studies::fetch()),
            StudiesCommand::Index { study_id } => write_raw(studies::fetch_index(study_id)),
        },
        Command::Account { command } => match command {
            AccountCommand::Roles => write_raw(account::fetch_roles()),
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
            ScheduleCommand::Terms => write_raw(schedule::fetch_terms()),
        },
        Command::Subjects { command } => match command {
            SubjectsCommand::Show {
                selector,
                from,
                to,
                news_since,
                max_news,
            } => {
                let explicit_route = selector.is_explicit_route();
                if let Err(code) = selector.validate() {
                    return code;
                }
                if explicit_route && (from.is_none() || to.is_none() || news_since.is_none()) {
                    eprintln!(
                        "studis: the explicit ID route requires --from, --to, and --news-since"
                    );
                    return ExitCode::from(2);
                }
                if let (Some(from), Some(to)) = (&from, &to)
                    && let Err(code) = validate_range(from, to)
                {
                    return code;
                }
                let mut request = selector.into_request();
                request.from = from;
                request.to = to;
                request.news_since = news_since;
                request.max_news = default_news_limit(explicit_route, max_news);
                match subject_view::fetch(request) {
                    Ok(view) => write_json(&view),
                    Err(message) => {
                        eprintln!("studis: {message}");
                        subject_error_exit_code(&message)
                    }
                }
            }
            SubjectsCommand::Files { selector } => {
                if let Err(code) = selector.validate() {
                    return code;
                }
                match moodle_files::fetch(selector.into_request()) {
                    Ok(files) => write_json(&files),
                    Err(message) => {
                        eprintln!("studis: {message}");
                        subject_error_exit_code(&message)
                    }
                }
            }
            SubjectsCommand::Download {
                file,
                output,
                selector,
            } => {
                if let Err(code) = selector.validate() {
                    return code;
                }
                match moodle_download::download(selector.into_request(), file.get(), &output) {
                    Ok(receipt) => write_json(&receipt),
                    Err(message) => {
                        eprintln!("studis: {message}");
                        subject_error_exit_code(&message)
                    }
                }
            }
        },
        Command::Auth { command } => match command {
            AuthCommand::Web { command } => match command {
                WebAuthCommand::Login { target } => match WebSession::open(true, true) {
                    Ok(session) => {
                        eprintln!("studis: Complete VUT sign-in in the browser window.");
                        match session.login(target) {
                            Ok(()) => write_json(
                                &serde_json::json!({"schema_version":1,"status":"signed_in"}),
                            ),
                            Err(error) => {
                                eprintln!("studis: {}", error.message());
                                ExitCode::FAILURE
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("studis: {}", error.message());
                        ExitCode::FAILURE
                    }
                },
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subject_defaults_preserve_explicit_news_cap_and_expand_shorthand() {
        assert_eq!(default_news_limit(true, None), 10);
        assert_eq!(default_news_limit(false, None), 0);
        assert_eq!(default_news_limit(true, Some(4)), 4);
        assert_eq!(default_news_limit(false, Some(4)), 4);
    }

    #[test]
    fn resolved_reversed_subject_window_is_an_argument_error() {
        assert_eq!(
            subject_error_exit_code("--from must not be after --to"),
            ExitCode::from(2)
        );
        assert_eq!(
            subject_error_exit_code("VUT API rate limited"),
            ExitCode::FAILURE
        );
    }

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
