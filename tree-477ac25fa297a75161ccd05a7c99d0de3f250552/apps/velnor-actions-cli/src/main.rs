//! `velnor-actions` command-line entry point.
//!
//! Owns argument parsing, typed dispatch, and exit codes. Must not own
//! orchestration algorithms, domain rules, or process creation.

mod args;
mod dispatch;
mod dispatch_config;
mod dispatch_gate;
mod dispatch_publish;
mod dispatch_verification_artifact;

use std::process::ExitCode;

/// Run the private gate on bare invocations, else the public Clap tree.
fn main() -> ExitCode {
    if std::env::args_os().len() <= 1
        && let Some(outcome) = dispatch::try_internal()
    {
        return outcome;
    }
    dispatch::run_public()
}
