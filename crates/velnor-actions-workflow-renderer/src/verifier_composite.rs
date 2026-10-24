//! Generator-owned composites for fixed tool-integrity verifier steps.
//!
//! The orchestrator supplies the canonical typed shell step. This module
//! renders it with the same credential scrub/unset validation as an ordinary
//! step and the fixed trusted interpreter used while cached bytes are on PATH.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{
    JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES, Step, StepKind, StepRole,
};

use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, yaml::render_yaml};

/// Fixed local action reference for Velnor's executable digest check.
pub const VERIFY_JACTIONLINT_USES: &str = JACTIONLINT_VERIFIER_USES;

/// Build one canonical verifier shell step from its standalone scrubbed script.
///
/// The public script helpers also serve the upgrades workflow directly, where
/// no shell-step constructor adds credential removal. Here the renderer
/// verifies and removes that exact leading prelude before delegating to
/// `shell_step`, which adds it once and owns the credential overlay.
///
/// # Errors
///
/// Returns [`RenderError`] when the identity, argv shape, or prelude is not
/// canonical.
pub fn verifier_shell_step(
    role: StepRole,
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if argv.len() != 3 || argv[0] != "sh" || argv[1] != "-c" {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_requires_canonical_sh_script".to_owned(),
        ));
    }
    match role {
        StepRole::VerifyRustManifest => {
            let prelude = format!("{} ", crate::toolchain_env::credential_unset_prelude());
            let Some(script) = argv[2].strip_prefix(&prelude).map(str::to_owned) else {
                return Err(RenderError::InvalidWorkflow(
                    "verifier_shell_script_missing_credential_prelude".to_owned(),
                ));
            };
            if script.starts_with(&prelude) {
                return Err(RenderError::InvalidWorkflow(
                    "verifier_shell_script_duplicate_credential_prelude".to_owned(),
                ));
            }
            let mut argv = argv;
            argv[2] = script;
            let mut step = crate::steps::shell_step(name, argv, env)?;
            step.role = Some(role);
            Ok(step)
        }
        StepRole::VerifyJactionlint => crate::steps::jactionlint_verifier_step(name, argv, env),
        _ => Err(RenderError::InvalidWorkflow(
            "nonverifier_role_for_verifier_step".to_owned(),
        )),
    }
}

/// Render the caller's typed shell step through the canonical startup boundary.
pub(crate) fn verifier_shell_step_to_yaml(step: &Step) -> Result<crate::yaml::Yaml, RenderError> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_requires_shell_step".to_owned(),
        ));
    };
    crate::verifier_shell::render_typed_step(step, run, env, &BTreeMap::new(), true)?
        .ok_or_else(|| RenderError::InvalidWorkflow("verifier_role_required".to_owned()))
}

/// Render one typed verifier as a complete workflow step mapping.
///
/// The verifier role controls admission. The display name remains presentation.
///
/// # Errors
///
/// Returns [`RenderError`] when the step lacks a verifier role or its typed
/// shell payload fails the canonical verifier contract.
pub fn render_verifier_workflow_step(
    step: &Step,
    job_env: &BTreeMap<String, String>,
) -> Result<crate::yaml::Yaml, RenderError> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_requires_shell_step".to_owned(),
        ));
    };
    crate::verifier_shell::render_typed_step(step, run, env, job_env, false)?
        .ok_or_else(|| RenderError::InvalidWorkflow("verifier_role_required".to_owned()))
}

/// Exact emitted `run:` value for a typed verifier shell step.
///
/// # Errors
///
/// Returns [`RenderError`] when `step` is not a valid typed verifier or its
/// emitted mapping does not contain a scalar `run` value.
pub fn verifier_shell_run_scalar(step: &Step) -> Result<String, RenderError> {
    let rendered = verifier_shell_step_to_yaml(step)?;
    let crate::yaml::Yaml::Map(fields) = rendered else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_step_render_not_mapping".to_owned(),
        ));
    };
    fields
        .into_iter()
        .find_map(|(key, value)| {
            if key != "run" {
                return None;
            }
            match value {
                crate::yaml::Yaml::Str(value)
                | crate::yaml::Yaml::Quoted(value)
                | crate::yaml::Yaml::Annotated { value, .. }
                | crate::yaml::Yaml::AnchoredScalar { value, .. } => Some(Ok(value)),
                _ => Some(Err(RenderError::InvalidWorkflow(
                    "verifier_run_not_scalar".to_owned(),
                ))),
            }
        })
        .unwrap_or_else(|| {
            Err(RenderError::InvalidWorkflow(
                "verifier_run_missing".to_owned(),
            ))
        })
}

/// Run the emitted body directly under the same fixed privileged interpreter.
///
/// # Errors
///
/// Returns [`RenderError`] when `step` cannot be rendered as a valid typed
/// verifier with a scalar `run` value.
pub fn verifier_shell_argv(step: &Step) -> Result<Vec<String>, RenderError> {
    let run = verifier_shell_run_scalar(step)?;
    Ok(vec![
        "/usr/bin/env".to_owned(),
        format!("PATH={}", crate::VERIFY_PATH),
        "/bin/bash".to_owned(),
        "--noprofile".to_owned(),
        "--norc".to_owned(),
        "-p".to_owned(),
        "-e".to_owned(),
        "-o".to_owned(),
        "pipefail".to_owned(),
        "-c".to_owned(),
        run,
    ])
}

/// Render one of the three canonical verification composites.
///
/// The shell's semantic role and trusted renderer establish authority. The
/// display name is preserved as presentation metadata only.
///
/// # Errors
///
/// Returns [`RenderError`] when the action identity, shell step shape,
/// generated bytes, or output path is invalid.
pub fn render_verifier_composite(
    uses: &str,
    step: &Step,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let (action_name, expected_role, expected_reference) = if uses == RUST_VERIFIER_USES[0] {
        (
            "Velnor restored Rust verifier",
            StepRole::VerifyRustManifest,
            "VELNOR_TOOL_RUST",
        )
    } else if uses == RUST_VERIFIER_USES[1] {
        (
            "Velnor Rust channel verifier",
            StepRole::VerifyRustManifest,
            "VELNOR_TOOL_RUST",
        )
    } else if uses == VERIFY_JACTIONLINT_USES {
        (
            "Velnor jactionlint executable verifier",
            StepRole::VerifyJactionlint,
            "VELNOR_TOOL_JACTIONLINT",
        )
    } else {
        return Err(RenderError::BadActionRef(format!(
            "unregistered_verifier:{uses}"
        )));
    };
    if step.role != Some(expected_role)
        || step.parallel
        || step.id.is_some()
        || step.condition.is_some()
        || !matches!(&step.kind, StepKind::Shell { .. })
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "noncanonical_verifier_step:{uses}"
        )));
    }
    validate_project_reference(step, expected_reference)?;

    let body = verifier_composite_document(action_name, step)?;
    let bytes = marker::with_marker(version, &render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    let logical = uses
        .strip_prefix("./.github/actions/")
        .ok_or_else(|| RenderError::BadActionRef(format!("nonlocal_verifier:{uses}")))?;
    let path = format!(".github/actions/{logical}/action.yml");
    crate::guard::validate_tree_path(&path)?;
    Ok(RenderedFile { path, bytes })
}

fn validate_project_reference(step: &Step, expected: &str) -> Result<(), RenderError> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_requires_shell_step".to_owned(),
        ));
    };
    let script = run.get(2).ok_or_else(|| {
        RenderError::InvalidWorkflow("verifier_composite_requires_canonical_sh_script".to_owned())
    })?;
    let references =
        velnor_actions_contract::config::project_tool_refs::project_tool_ref_names(script)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
    if !references.is_empty() && references != std::collections::BTreeSet::from([expected]) {
        return Err(RenderError::InvalidWorkflow(
            "verifier_composite_tool_binding".to_owned(),
        ));
    }
    Ok(())
}

fn verifier_composite_document(
    action_name: &str,
    step: &Step,
) -> Result<crate::yaml::Yaml, RenderError> {
    let shell = verifier_shell_step_to_yaml(step)?;
    crate::composite::composite_yaml(action_name, vec![shell])
}

#[cfg(test)]
#[path = "verifier_composite_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "verifier_composite_main_tests.rs"]
mod main_tests;
