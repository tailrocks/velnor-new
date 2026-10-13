//! Validator install/exec closure checks for the renderer context.

use std::collections::BTreeSet;

use crate::{RenderError, render::ValidatorCommand};

/// Every tool a validator executes must be explicitly installed in that job.
///
/// A successful tools-cache restore may provide the install, but a miss must
/// still work with automatic installation disabled. This checks both the
/// command's own install vector and its separate preparation vector.
pub(super) fn validate_validator_tool_closure(
    command: &ValidatorCommand,
) -> Result<(), RenderError> {
    let mut installed = BTreeSet::new();
    for (verb, spec) in validator_operations(&command.prepare_argv) {
        if verb != "install" {
            return Err(RenderError::BadCommand(format!(
                "validator_prepare_not_install:{}:{verb}",
                command.validator.job_id()
            )));
        }
        installed.insert(spec);
    }
    for (verb, spec) in validator_operations(&command.argv) {
        if verb == "install" {
            installed.insert(spec);
        } else if verb == "exec" && !installed.contains(&spec) {
            return Err(RenderError::BadCommand(format!(
                "validator_install_missing:{}:{spec}",
                command.validator.job_id()
            )));
        }
    }
    Ok(())
}

/// Exact Mise operations in command order. Preparation runs before the
/// command, while an install in a combined command helps only when it
/// precedes its exec.
fn validator_operations(argv: &[String]) -> Vec<(String, String)> {
    let mut operations = Vec::new();
    let mut verb = None;
    for word in crate::cache_p08_detect::detector_words(argv) {
        if matches!(word.as_str(), "install" | "exec") {
            verb = Some(word);
            continue;
        }
        if word == "--" || matches!(word.as_str(), "&&" | ";" | "||" | "}" | "{") {
            verb = None;
            continue;
        }
        if let Some(active) = &verb
            && crate::cache_p08::is_tool_spec(&word)
        {
            operations.push((active.clone(), word));
        }
    }
    operations
}

#[cfg(test)]
mod tests {
    use velnor_actions_contract::ValidatorKind;

    use super::validate_validator_tool_closure;
    use crate::render::ValidatorCommand;

    fn command(prepare_argv: Vec<&str>) -> ValidatorCommand {
        ValidatorCommand {
            validator: ValidatorKind::Zizmor,
            name: "Run zizmor".to_owned(),
            prepare_argv: prepare_argv.into_iter().map(str::to_owned).collect(),
            argv: [
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                "zizmor@1.30.1",
                "--",
                "zizmor",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }

    #[test]
    fn validator_exec_requires_matching_explicit_install() {
        assert!(validate_validator_tool_closure(&command(Vec::new())).is_err());
        assert!(
            validate_validator_tool_closure(&command(vec![
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "install",
                "actionlint@1.7.12",
            ]))
            .is_err()
        );
        assert!(
            validate_validator_tool_closure(&command(vec![
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "install",
                "zizmor@1.30.1",
            ]))
            .is_ok()
        );
    }

    #[test]
    fn same_command_install_must_precede_exec() {
        let mut command = command(Vec::new());
        command.argv = [
            "mise",
            "--no-config",
            "exec",
            "zizmor@1.30.1",
            "--",
            "zizmor",
            "&&",
            "mise",
            "--no-config",
            "install",
            "zizmor@1.30.1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(validate_validator_tool_closure(&command).is_err());
        command.argv = [
            "mise",
            "--no-config",
            "install",
            "zizmor@1.30.1",
            "&&",
            "mise",
            "--no-config",
            "exec",
            "zizmor@1.30.1",
            "--",
            "zizmor",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(validate_validator_tool_closure(&command).is_ok());
    }

    #[test]
    fn preparation_vectors_cannot_execute_tools() {
        let mut command = command(Vec::new());
        command.prepare_argv = [
            "mise",
            "exec",
            "zizmor@1.30.1",
            "--",
            "zizmor",
            "&&",
            "mise",
            "install",
            "zizmor@1.30.1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(validate_validator_tool_closure(&command).is_err());
        command.prepare_argv = ["mise", "install", "zizmor@1.30.1"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(validate_validator_tool_closure(&command).is_ok());
    }
}
