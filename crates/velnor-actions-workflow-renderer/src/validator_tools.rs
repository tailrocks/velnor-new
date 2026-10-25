use std::collections::BTreeSet;

use crate::{RenderError, render::ValidatorCommand};

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
