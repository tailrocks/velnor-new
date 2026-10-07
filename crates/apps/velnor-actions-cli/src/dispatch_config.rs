//! `velnor-actions config` dispatch.
//!
//! Migration preview prints one document. `--write` stores those same bytes.

use std::process::ExitCode;

use velnor_actions_orchestrator::migrate_config;
use velnor_actions_orchestrator_core::resolve_root;

use crate::args::ConfigCommand;
use crate::dispatch::{fail_public, working_dir};

/// Dispatch one config subcommand.
pub(crate) fn run_config(command: &ConfigCommand) -> ExitCode {
    match command {
        ConfigCommand::Migrate { to, write } => run_migrate(*to, *write),
    }
}

/// Print the schema 2 config. Persist it only when `write` is set.
fn run_migrate(to: u32, write: bool) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    match migrate_config(&root, to, write) {
        Ok(text) => {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
