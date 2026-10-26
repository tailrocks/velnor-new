// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use velnor_actions_orchestrator::{migrate_config, resolve_root};

use crate::args::ConfigCommand;
use crate::dispatch::{fail_public, working_dir};

pub(crate) fn run_config(command: &ConfigCommand) -> ExitCode {
    match command {
        ConfigCommand::Migrate { to, write } => run_migrate(*to, *write),
    }
}

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
