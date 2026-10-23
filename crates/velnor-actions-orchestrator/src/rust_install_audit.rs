//! Generate-time coverage audit: every generated workflow and composite
//! action that installs or changes a Rust toolchain verifies it before use.
//!
//! This audit reads rendered `.github/**/*.yml` text, so no renderer can add a
//! Rust install that skips verification. Typed local verifier calls are bound
//! to their exact action reference and definition; legacy inline verifier
//! scripts are accepted only when their full emitted mapping matches a
//! canonical template.

#[cfg(test)]
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{render::RenderedFile, yaml::quote_scalar};

use crate::OrchestratorError;
use crate::workflow::verifier_actions::{
    InlineVerifierTemplate, VerifierAction, VerifierActionSet,
};

#[path = "rust_install_audit_parse.rs"]
mod parse;
use parse::{StepText, step_lists};

/// Text that installs or changes a Rust toolchain.
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

/// Text of a step that runs Rust, Cargo or MBX.
const USES: [&str; 6] = [
    "exec 'rust@",
    "exec rust@",
    "-- rustc",
    "-- cargo",
    "-- mbx",
    "rustc --print",
];

/// Inside an install step: it already runs what it installs (`rustup`
/// itself, for components and targets, is allowed).
const RUNS: [&str; 4] = ["-- rustc", "-- cargo", "-- mbx", "rustc --print"];

/// A shell step that installs Rust and verifies it inside the same script
/// (`upgrades.yml`) calls this between the install and any use.
const SELF_VERIFY: &str = "$VELNOR_VERIFY_RUST_SCRIPT";
const VERIFIER_ACTION_PREFIX: &str = ".github/actions/velnor-verify-";

/// Fail generation when a rendered file installs Rust without verifying it.
///
/// # Errors
///
/// Returns a contract error when verifier definitions/calls are changed or a
/// Rust install is not followed by its canonical verification step.
#[cfg(test)]
pub(crate) fn audit(files: &[RenderedFile]) -> Result<(), OrchestratorError> {
    let verifiers = VerifierActionSet::build(
        &ToolCatalog::pinned().for_emission(),
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: format!("verifier_action_templates_invalid:{error}"),
    })?;
    audit_with_verifiers(files, &verifiers)
}

/// Validate all rendered Rust installs against exact verifier definitions
/// generated from the same catalog as the workflow IR.
///
/// # Errors
///
/// Returns a contract error for an install without its canonical check, an
/// altered/missing verifier action definition, or a malformed typed call.
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
        for (owner, steps) in step_lists(&file.bytes) {
            let path = format!("{}:{owner}", file.path);
            for step in &steps {
                if step.invalid_anchor {
                    return Err(unverified(
                        &path,
                        &step.name,
                        "invalid YAML anchor declaration or alias",
                    ));
                }
                if !is_verifier_step_valid(step, verifiers) {
                    return Err(unverified(
                        &path,
                        &step.name,
                        "noncanonical verifier call or body",
                    ));
                }
            }
            check_steps(&path, &steps, verifiers)?;
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
            .strip_prefix(VERIFIER_ACTION_PREFIX)
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
    if step.is_verifier_ref_candidate() {
        return step
            .uses_value()
            .and_then(|uses| verifiers.by_uses(uses))
            .is_some_and(|action| canonical_action_call(step, action));
    }
    if verifiers
        .inline_rust_templates
        .values()
        .flatten()
        .chain(&verifiers.inline_jactionlint_templates)
        .any(|template| canonical_inline_shape(step, template))
    {
        return true;
    }
    !claims_verifier_label(step, verifiers)
}

fn claims_verifier_label(step: &StepText, verifiers: &VerifierActionSet) -> bool {
    step.fields.iter().any(|(key, value)| {
        key == "name"
            && [
                verifiers.restored_rust.name,
                verifiers.installed_rust.name,
                verifiers.jactionlint.name,
            ]
            .contains(&value.trim().trim_matches('"').trim_matches('\''))
    })
}

fn canonical_inline_shape(step: &StepText, template: &InlineVerifierTemplate) -> bool {
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
    let Some(actual_name) = step.field("name") else {
        return false;
    };
    if step.name.trim().is_empty() {
        return false;
    }

    let mut expected_top = template
        .top_fields
        .iter()
        .filter(|(key, _)| key != "env")
        .cloned()
        .collect::<Vec<_>>();
    let Some((_, name)) = expected_top.iter_mut().find(|(key, _)| key == "name") else {
        return false;
    };
    actual_name.clone_into(name);
    let actual_top = step
        .fields
        .iter()
        .filter(|(key, _)| key != "env")
        .cloned()
        .collect::<Vec<_>>();
    let expected_shell = quote_scalar(shell);
    if actual_top != expected_top
        || step.field("shell") != Some(expected_shell.as_str())
        || step.field("run") != Some(template.run_scalar.as_str())
        || step.field("env").is_none()
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
    actual_env == expected_env
}

fn canonical_action_call(step: &StepText, action: &VerifierAction) -> bool {
    !step.unexpected_nested
        && step.fields.len() == 2
        && step.fields[0].0 == "name"
        && !step.name.trim().is_empty()
        && step.fields[0].1 == quote_scalar(&step.name)
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
        // The install run, then steps that never touch Rust (artifact
        // downloads, the Velnor helper), then the verification.
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
            if USES.iter().any(|needle| step_script(step).contains(needle)) && !installs_rust(step)
                || installs_rust(step)
                    && RUNS.iter().any(|needle| step_script(step).contains(needle))
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

/// The step verifies its own install: the check runs after every install in
/// its script and before every use of Rust, Cargo or MBX.
fn self_verified(step: &StepText) -> bool {
    let text = step_script(step);
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
    INSTALLS
        .iter()
        .any(|needle| step_script(step).contains(needle))
}

fn step_script(step: &StepText) -> &str {
    step.run_scalar.as_deref().unwrap_or(&step.text)
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

#[cfg(test)]
#[path = "rust_install_audit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "rust_install_audit_verifier_action_tests.rs"]
mod verifier_action_tests;
