// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::{render::RenderedFile, yaml::quote_scalar};

use crate::OrchestratorError;
use crate::workflow::verifier_actions::{VerifierAction, VerifierActionSet};

#[path = "rust_install_audit_parse.rs"]
mod parse;
use parse::{StepText, step_lists};

const INSTALLS: [&str; 9] = [
    " install 'rust@",
    " install rust@",
    " install \"rust@",
    " install \\\"rust@",
    "rustup toolchain install",
    "rustup component add",
    "rustup target add",
    "rustup-init",
    "rustup toolchain link",
];

const USES: [&str; 6] = [
    "exec 'rust@",
    "exec rust@",
    "-- rustc",
    "-- cargo",
    "-- mbx",
    "rustc --print",
];

const RUNS: [&str; 4] = ["-- rustc", "-- cargo", "-- mbx", "rustc --print"];

const SELF_VERIFY: &str = "$VELNOR_VERIFY_RUST_SCRIPT";

pub(crate) fn audit_with_verifiers(
    files: &[RenderedFile],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    check_verifier_files(files, verifiers)?;
    for file in files {
        let yaml = std::path::Path::new(&file.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if !(file.path.starts_with(".github/") && yaml) {
            continue;
        }
        if let Some(anchor) = parse::duplicate_anchor_names(&file.bytes).iter().next() {
            return Err(unverified(
                &file.path,
                anchor,
                "duplicate YAML anchor in document",
            ));
        }
        for (owner, steps) in step_lists(&file.bytes) {
            for step in &steps {
                if step.invalid_anchor {
                    return Err(unverified(
                        &format!("{}:{owner}", file.path),
                        &step.name,
                        "unresolved or malformed YAML anchor",
                    ));
                }
                if !is_verifier_step_valid(step, verifiers) {
                    return Err(unverified(
                        &format!("{}:{owner}", file.path),
                        &step.name,
                        "noncanonical verifier call or body",
                    ));
                }
            }
            check_steps(&format!("{}:{owner}", file.path), &steps, verifiers)?;
        }
    }
    Ok(())
}

fn check_verifier_files(
    files: &[RenderedFile],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    let expected = verifiers.files();
    let expected_paths: std::collections::BTreeSet<&str> =
        expected.iter().map(|file| file.path.as_str()).collect();
    for action in [
        &verifiers.restored_rust,
        &verifiers.installed_rust,
        &verifiers.jactionlint,
    ] {
        let matching: Vec<_> = files
            .iter()
            .filter(|file| file.path == action.file.path)
            .collect();
        if matching.len() != 1 {
            return Err(unverified(
                &action.file.path,
                action.name,
                "missing or duplicate verifier action definition",
            ));
        }
        if matching[0].bytes != action.file.bytes {
            return Err(unverified(
                &action.file.path,
                action.name,
                "verifier action body, shell, environment, or marker changed",
            ));
        }
    }
    for file in files.iter().filter(|file| {
        file.path
            .strip_prefix(".github/actions/velnor-verify-")
            .is_some_and(|_| file.path.ends_with("/action.yml"))
    }) {
        if !expected_paths.contains(file.path.as_str()) {
            return Err(unverified(
                &file.path,
                "unregistered verifier action",
                "unexpected generated verifier definition",
            ));
        }
    }
    Ok(())
}

fn is_verifier_step_valid(step: &StepText, verifiers: &VerifierActionSet) -> bool {
    if let Some(uses) = step.uses_value()
        && let Some(action) = verifiers.by_uses(uses)
    {
        return canonical_action_call(step, action);
    }
    let known_inline_name = verifiers
        .inline_rust_templates
        .contains_key(step.name.as_str())
        || step.name == verifiers.jactionlint.name;
    let matches = verifiers
        .inline_rust_templates
        .values()
        .flatten()
        .chain(&verifiers.inline_jactionlint_templates)
        .any(|template| canonical_inline_shape(step, template));
    if matches {
        return true;
    }
    !known_inline_name
}

fn canonical_inline_shape(
    step: &StepText,
    template: &crate::workflow::verifier_actions::InlineVerifierTemplate,
) -> bool {
    if step.unexpected_nested
        || template
            .required_env
            .iter()
            .any(|key| !template.env.contains_key(*key))
    {
        return false;
    }
    let Some(shell) = template.shell else {
        return false;
    };
    let mut expected_top = template.top_fields.clone();
    let Some((_, name)) = expected_top.iter_mut().find(|(key, _)| key == "name") else {
        return false;
    };
    if step.name.trim().is_empty() {
        return false;
    }
    *name = quote_scalar(&step.name);
    if !expected_top
        .iter()
        .any(|(key, value)| key == "shell" && value == &quote_scalar(shell))
        || !expected_top
            .iter()
            .any(|(key, value)| key == "run" && value == &template.run_scalar)
    {
        return false;
    }
    let mut actual_env = step.env_fields.clone();
    actual_env.sort_unstable();
    let expected_env = template
        .env
        .iter()
        .map(|(key, value)| (key.clone(), quote_scalar(value)))
        .collect::<Vec<_>>();
    step.top_fields == expected_top && actual_env == expected_env
}

fn canonical_action_call(step: &StepText, action: &VerifierAction) -> bool {
    !step.unexpected_nested
        && step.fields.len() == 2
        && step.fields[0].0 == "name"
        && !step.fields[0].1.trim().is_empty()
        && step.fields[0].1 == step.name
        && step.fields[1].0 == "uses"
        && step.fields[1].1 == format!("{} # zizmor: ignore[self-repository]", action.uses)
}

fn unverified(path: &str, step: &str, why: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("rust_install_unverified:{path}:{step}:{why}"),
    }
}

fn check_steps(
    path: &str,
    steps: &[StepText],
    verifiers: &VerifierActionSet,
) -> Result<(), OrchestratorError> {
    let mut index = 0;
    while index < steps.len() {
        if !installs_rust(&steps[index]) {
            index += 1;
            continue;
        }
        if self_verified(&steps[index]) {
            index += 1;
            continue;
        }
        let start = index;
        loop {
            let Some(step) = steps.get(index) else {
                return Err(unverified(
                    path,
                    &steps[start].name,
                    "no verification follows the install",
                ));
            };
            if is_rust_verifier(step, verifiers) {
                break;
            }
            if USES.iter().any(|needle| step.text.contains(needle)) && !installs_rust(step)
                || installs_rust(step) && RUNS.iter().any(|needle| step.text.contains(needle))
            {
                return Err(unverified(
                    path,
                    &step.name,
                    "runs the toolchain it installs before the verification",
                ));
            }
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

fn self_verified(step: &StepText) -> bool {
    let text = step.run_scalar.as_deref().unwrap_or(&step.text);
    let Some(check) = text.find(SELF_VERIFY) else {
        return false;
    };
    let last_install = INSTALLS
        .iter()
        .filter_map(|needle| text.rfind(needle))
        .max()
        .unwrap_or(0);
    let first_use = USES
        .iter()
        .filter_map(|needle| text.find(needle))
        .min()
        .unwrap_or(usize::MAX);
    last_install < check && check < first_use
}

fn installs_rust(step: &StepText) -> bool {
    INSTALLS.iter().any(|needle| step.text.contains(needle))
}

fn is_rust_verifier(step: &StepText, verifiers: &VerifierActionSet) -> bool {
    if step.uses_value() == Some(verifiers.installed_rust.uses) {
        return canonical_action_call(step, &verifiers.installed_rust);
    }
    verifiers
        .inline_installed_rust_templates
        .iter()
        .any(|template| canonical_inline_shape(step, template))
}
