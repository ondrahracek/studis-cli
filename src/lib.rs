//! Internal modules for the Studis CLI.

mod auth;
pub mod cli;
mod dates;
mod http;
mod moodle_download;
mod moodle_files;
mod moodle_url;
mod resources;
mod subject_view;
mod token_store;
#[cfg(unix)]
mod unix_acl;
mod web;
mod web_session;
