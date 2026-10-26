// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use velnor_actions_orchestrator::{lock_playwright_browsers, lock_private_sources, resolve_root};

use crate::args::{DepsCommand, PlaywrightCommand};
use crate::dispatch::{fail_public, working_dir};

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

pub(crate) fn run_playwright(command: &PlaywrightCommand) -> ExitCode {
    match command {
        PlaywrightCommand::Lock => run_playwright_lock(),
    }
}

fn run_playwright_lock() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    match resolve_root(&cwd).and_then(|root| lock_playwright_browsers(&root)) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
