//! Generate-time coverage audit: every generated workflow and composite
//! action that installs or changes a Rust toolchain verifies it before use.
//!
//! The lock audit reads only the `ci.yml` IR; release, qualification,
//! freshness, upgrade and composite-action files are rendered directly. This
//! audit reads every rendered `.github/**/*.yml` text instead, so no
//! renderer can add a Rust install that skips the check. For each `steps:`
//! list (YAML step anchors resolved) every step that installs or changes
//! Rust must be followed by [`VERIFY_RUST_TOOLCHAIN_STEP`] before any step
//! runs Rust, Cargo or MBX, and no install step may already run them.

#[cfg(test)]
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{render::RenderedFile, yaml::quote_scalar};

use crate::OrchestratorError;
use crate::verifier_actions::{VerifierAction, VerifierActionSet};

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

/// Fail generation when a rendered file installs Rust without verifying it.
///
/// # Errors
///
/// Returns a contract error `rust_install_unverified:<path>:<step>`.
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

/// Validate all rendered Rust installs against exact verifier call/file
/// templates generated from the same catalog as the workflow IR.
///
/// # Errors
///
/// Returns a contract error for an install without its canonical check or an
/// altered/missing verifier action definition or call.
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
            for step in &steps {
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
    if let Some(templates) = verifiers.inline_rust_templates.get(step.name.as_str()) {
        let matches = templates
            .iter()
            .any(|template| canonical_inline_shape(step, step.name.as_str(), template));
        return matches;
    }
    if step.name == verifiers.jactionlint.name {
        return verifiers
            .inline_jactionlint_templates
            .iter()
            .any(|template| canonical_inline_shape(step, verifiers.jactionlint.name, template));
    }
    true
}

fn canonical_inline_shape(
    step: &StepText,
    name: &str,
    template: &crate::verifier_actions::InlineVerifierTemplate,
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
    let expected_top = vec![
        ("name".to_owned(), quote_scalar(name)),
        ("env".to_owned(), String::new()),
        ("shell".to_owned(), quote_scalar(shell)),
        ("run".to_owned(), template.run_scalar.clone()),
    ];
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
    step.name == action.name
        && step.fields.len() == 2
        && step.fields[0].0 == "name"
        && step.fields[0].1 == action.name
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

/// The step verifies its own install: the check runs after every install in
/// its text and before every use of Rust, Cargo or MBX.
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
        .inline_rust_templates
        .get(step.name.as_str())
        .is_some_and(|templates| {
            templates
                .iter()
                .any(|template| canonical_inline_shape(step, &step.name, template))
        })
}

#[cfg(test)]
#[path = "rust_install_audit_tests.rs"]
mod tests;
