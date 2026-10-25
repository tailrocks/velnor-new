//! Public command dispatch and user-facing error formatting.

use std::process::ExitCode;

use clap::Parser;
use velnor_actions_orchestrator::{init_config, plan_text_checked, prepare, resolve_root};

use super::{fail_public, owned_publication, working_dir};
use crate::args::{Cli, Command};

/// Parse Clap arguments and dispatch one public command.
///
/// Clap owns `--help`, `--version`, and usage errors (exit 2).
pub(crate) fn run_public() -> ExitCode {
    let command = Cli::parse().command;
    if command.is_owned_preview() {
        return owned_publication::run_owned_command(command);
    }
    match command {
        Command::Init => run_init(),
        Command::Plan => run_plan(),
        Command::Generate {
            output_dir,
            mode,
            job_image_candidate_only: true,
            ..
        } => crate::dispatch_job_image_preview::run(output_dir, mode.as_deref()),
        Command::Generate {
            output_dir, mode, ..
        } => crate::dispatch_generate::run_generate(output_dir, mode),
        Command::Unmanaged => crate::dispatch_unmanaged::run_unmanaged(),
        Command::Config { command } => crate::dispatch_config::run_config(&command),
        Command::Deps { command } => crate::dispatch_deps::run_deps(&command),
        Command::Playwright { command } => crate::dispatch_deps::run_playwright(&command),
        Command::Homebrew { command } => crate::dispatch_homebrew_adoption::run(&command),
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
        Command::AuthorityInventory => crate::dispatch_job_authority::run(),
    }
}

/// Dispatch `init`: resolve the root, then create the config file.
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

/// Dispatch `plan`: report (recommendations included) to stdout only.
///
/// Contract §5 routes findings to stderr in one sentence, but §5's own
/// example shows `Recommendations` inside the stdout report and §7 assigns
/// the report to stdout; the example plus §7 govern, so stderr stays empty.
fn run_plan() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let preparation = resolve_root(&cwd).and_then(|root| prepare(&root));
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
