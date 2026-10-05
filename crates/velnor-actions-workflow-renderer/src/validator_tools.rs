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
    let mut installed = validator_specs(&command.prepare_argv, "install");
    installed.extend(validator_specs(&command.argv, "install"));
    let executed = validator_specs(&command.argv, "exec");
    for spec in executed {
        if !installed.contains(&spec) {
            return Err(RenderError::BadCommand(format!(
                "validator_install_missing:{}:{spec}",
                command.validator.job_id()
            )));
        }
    }
    Ok(())
}

/// Exact Mise selectors belonging to one command verb.
fn validator_specs(argv: &[String], verb: &str) -> BTreeSet<String> {
    let mut specs = BTreeSet::new();
    let mut collect = false;
    for word in crate::cache_p08_detect::detector_words(argv) {
        if word == "install" || word == "exec" {
            collect = word == verb;
            continue;
        }
        if word == "--" || matches!(word.as_str(), "&&" | ";" | "||" | "}" | "{") {
            collect = false;
            continue;
        }
        if collect && crate::cache_p08::is_tool_spec(&word) {
            specs.insert(word);
        }
    }
    specs
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
}
