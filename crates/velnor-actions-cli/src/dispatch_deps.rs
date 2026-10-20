//! `velnor-actions deps` dispatch.
//!
//! `lock` is the only writer of `.velnor/deps.lock`; it prints the document
//! it stored.

use std::process::ExitCode;

use velnor_actions_orchestrator::{lock_private_sources, resolve_root};

use crate::args::DepsCommand;
use crate::dispatch::{fail_public, working_dir};

/// Dispatch one deps subcommand.
pub(crate) fn run_deps(command: &DepsCommand) -> ExitCode {
    match command {
        DepsCommand::Lock => run_lock(),
    }
}

fn run_lock() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    match resolve_root(&cwd).and_then(|root| lock_private_sources(&root)) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
