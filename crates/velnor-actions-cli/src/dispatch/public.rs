// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use clap::Parser;
use velnor_actions_orchestrator::{
    OperatorToolchainGuard, OrchestratorError, init_config, plan_text_checked, prepare,
    prepare_with_operator_guard, resolve_root,
};

use super::{fail_public, owned_publication, working_dir};
use crate::args::{Cli, Command};

pub(crate) fn run_public() -> ExitCode {
    let cli = Cli::parse();
    let guard = match cli.toolchain_guard {
        Some(path) => match OperatorToolchainGuard::new(path) {
            Ok(guard) => Some(guard),
            Err(error) => {
                return fail_public(&OrchestratorError::ToolchainGuard {
                    problem: error.to_string(),
                });
            }
        },
        None => None,
    };
    let command = cli.command;
    if guard.is_some() && !supports_operator_guard(&command) {
        return fail_public(&OrchestratorError::ToolchainGuard {
            problem: "operator toolchain guard is unsupported for this command".to_owned(),
        });
    }
    if command.is_owned_preview() {
        return owned_publication::run_owned_command(command);
    }
    match command {
        Command::Init => run_init(),
        Command::Plan => run_plan(guard.as_ref()),
        Command::Generate {
            output_dir, mode, ..
        } => crate::dispatch_generate::run_generate(output_dir, mode, guard.as_ref()),
        Command::Unmanaged => crate::dispatch_unmanaged::run_unmanaged(),
        Command::Config { command } => crate::dispatch_config::run_config(&command),
        Command::Deps { command } => crate::dispatch_deps::run_deps(&command),
        Command::Playwright { command } => crate::dispatch_deps::run_playwright(&command),
        Command::Homebrew { command } => {
            crate::dispatch_homebrew_adoption::run(&command, guard.as_ref())
        }
        Command::ImageMaintenance { command } => crate::dispatch_image_maintenance::run(&command),
        Command::QualifyTools {
            request,
            work_dir,
            output,
        } => crate::dispatch_qualify_tools::run(&request, &work_dir, &output),
        Command::VerifyReleaseManifest {
            manifest,
            expected_source_commit,
            linux_x64_binary,
            macos_arm64_binary,
        } => crate::dispatch_local_release::run_verify_release_manifest(
            &manifest,
            &expected_source_commit,
            &linux_x64_binary,
            &macos_arm64_binary,
        ),
    }
}

fn supports_operator_guard(command: &Command) -> bool {
    matches!(
        command,
        Command::Plan
            | Command::Generate {
                owned_tool_candidates_only: false,
                ..
            }
            | Command::Homebrew {
                command: crate::args::HomebrewCommand::SourcePolicy { .. }
            }
    )
}

fn run_init() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let report = resolve_root(&cwd).and_then(|root| init_config(&root));
    match report {
        Ok(report) => {
            for path in &report.created {
                println!("{path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}

fn run_plan(guard: Option<&OperatorToolchainGuard>) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let preparation = resolve_root(&cwd).and_then(|root| match guard {
        Some(guard) => prepare_with_operator_guard(&root, guard),
        None => prepare(&root),
    });
    let preparation = match preparation {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let text = match plan_text_checked(&preparation) {
        Ok(text) => text,
        Err(error) => return fail_public(&error),
    };
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    ExitCode::SUCCESS
}
