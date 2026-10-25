//! Thin public CLI adapter for native Homebrew adoption authoring.

use std::{path::Path, process::ExitCode};

use crate::args::HomebrewCommand;
use crate::dispatch::fail_public;

pub(crate) fn run(command: &HomebrewCommand) -> ExitCode {
    let report = match command {
        HomebrewCommand::SourcePolicy {
            root,
            output,
            check,
        } => {
            let result = if *check {
                velnor_actions_orchestrator::check_homebrew_source_policy(Path::new(root))
            } else if let Some(output) = output {
                velnor_actions_orchestrator::export_homebrew_source_policy(
                    Path::new(root),
                    Path::new(output),
                )
            } else {
                return fail_public(&velnor_actions_orchestrator::OrchestratorError::Internal {
                    problem: "homebrew_source_policy_mode_required".to_owned(),
                });
            };
            return match result {
                Ok(()) => {
                    if *check {
                        println!("checked .velnor/homebrew-release-policy.json");
                    } else if let Some(output) = output {
                        println!("{}", output.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => fail_public(&error),
            };
        }
        HomebrewCommand::AdoptionSeed {
            transition,
            sdk_manifest,
            output_dir,
        } => velnor_actions_orchestrator::author_homebrew_adoption_seed(
            transition,
            Path::new(sdk_manifest),
            Path::new(output_dir),
        ),
        HomebrewCommand::AdoptionCutover {
            transition,
            sdk_manifest,
            output_dir,
        } => velnor_actions_orchestrator::author_homebrew_adoption_cutover(
            transition,
            Path::new(sdk_manifest),
            Path::new(output_dir),
        ),
    };
    match report {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::from(1),
        },
        Err(error) => fail_public(&error),
    }
}
