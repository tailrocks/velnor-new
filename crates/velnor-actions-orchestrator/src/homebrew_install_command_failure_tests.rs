use std::path::PathBuf;

use super::{CommandFailurePhase, command_failure};
use crate::homebrew::command::HomebrewCommand;

#[test]
fn all_closed_operations_have_fixed_diagnostic_labels() {
    let operations = [
        (HomebrewCommand::Version, "version"),
        (HomebrewCommand::Config, "config"),
        (
            HomebrewCommand::InstallPortableRuby,
            "install-portable-ruby",
        ),
        (
            HomebrewCommand::MovePrefix {
                source: PathBuf::from("/secret/source"),
                destination: PathBuf::from("/secret/destination"),
            },
            "move-prefix",
        ),
        (HomebrewCommand::Prefix, "prefix"),
        (HomebrewCommand::Cellar, "cellar"),
        (
            HomebrewCommand::InstallCandidate {
                recipe: PathBuf::from("/secret/formula.rb"),
            },
            "install-candidate",
        ),
        (
            HomebrewCommand::UpgradeCandidate {
                recipe: PathBuf::from("/secret/formula.rb"),
            },
            "upgrade-candidate",
        ),
        (
            HomebrewCommand::TestArchived {
                recipe: PathBuf::from("/secret/formula.rb"),
            },
            "test-archived",
        ),
        (
            HomebrewCommand::InstalledBinaryVersion {
                executable: PathBuf::from("/secret/binary"),
            },
            "installed-binary-version",
        ),
    ];

    for (operation, expected) in operations {
        assert_eq!(operation.diagnostic_label(), expected);
    }
}

#[test]
fn failure_format_reports_stage_and_safe_exit_code_only() {
    let cases = [
        (
            CommandFailurePhase::ConstructionRejected,
            None,
            "phase=construction-rejected",
        ),
        (
            CommandFailurePhase::ExecutionFailed,
            None,
            "phase=execution-failed",
        ),
        (
            CommandFailurePhase::NonzeroExit,
            Some(37),
            "phase=nonzero-exit exit=37",
        ),
    ];

    for (phase, exit_code, expected) in cases {
        let error = command_failure("install-candidate", phase, exit_code).to_string();
        assert!(error.contains("operation=install-candidate"));
        assert!(error.contains(expected));
        assert!(!error.contains("SECRET"));
        assert!(!error.contains("/private/"));
    }
}

#[test]
fn failure_format_never_discloses_operation_paths_or_recipe_values() {
    let operation = HomebrewCommand::InstallCandidate {
        recipe: PathBuf::from("/private/tmp/SECRET-formula.rb"),
    };
    let error = command_failure(
        operation.diagnostic_label(),
        CommandFailurePhase::ExecutionFailed,
        None,
    )
    .to_string();

    assert_eq!(
        error,
        "internal: homebrew_install_command_failed operation=install-candidate phase=execution-failed"
    );
    assert!(!error.contains("SECRET"));
    assert!(!error.contains("formula.rb"));
    assert!(!error.contains("/private/tmp"));
}
