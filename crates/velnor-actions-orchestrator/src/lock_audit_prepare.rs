//! Prepare/validator install inputs and their verifier-sequence checks.

use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_mise::toolfiles::lockfile::InstallSubject;
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::workflow::verifier_actions::{VerifierAction, VerifierActionSet, VerifierSteps};

use super::{
    CommandInstalls, MiseVectors, validator_argv_specs, validator_subject,
    vectors::classify_mise_vectors, verifiers,
};

#[derive(Default)]
pub(super) struct VerifierPositions {
    restored_rust: Vec<usize>,
    installed_rust: Vec<usize>,
    jactionlint: Vec<usize>,
}

pub(super) fn verifier_positions(
    id: &str,
    job: &velnor_actions_contract::Job,
    verifiers: &VerifierActionSet,
    inline_verifiers: Option<&VerifierSteps>,
    blocking: &mut Vec<String>,
) -> VerifierPositions {
    let mut positions = VerifierPositions::default();
    for (index, step) in job.steps.iter().enumerate() {
        if let StepKind::Action { uses, .. } = &step.kind
            && let Some(action) = verifiers.by_uses(uses)
        {
            if !VerifierActionSet::canonical_call(step, action) {
                blocking.push(format!("noncanonical_verifier_call:{id}:{}", action.uses));
                continue;
            }
            record_action(&mut positions, index, action, verifiers);
            continue;
        }
        if verifiers::restored_rust(step, inline_verifiers) {
            positions.restored_rust.push(index);
            continue;
        }
        if verifiers::installed_rust(step, inline_verifiers) {
            positions.installed_rust.push(index);
            continue;
        }
        if verifiers::jactionlint(step, inline_verifiers) {
            positions.jactionlint.push(index);
            continue;
        }
        if claims_verifier(step) {
            blocking.push(format!("forged_verifier_call:{id}:{}", step.name));
        }
    }
    if positions.restored_rust.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:restored-rust"));
    }
    if positions.installed_rust.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:installed-rust"));
    }
    if positions.jactionlint.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:jactionlint"));
    }
    positions
}

fn record_action(
    positions: &mut VerifierPositions,
    index: usize,
    action: &VerifierAction,
    verifiers: &VerifierActionSet,
) {
    if action.uses == verifiers.restored_rust.uses {
        positions.restored_rust.push(index);
    } else if action.uses == verifiers.installed_rust.uses {
        positions.installed_rust.push(index);
    } else if action.uses == verifiers.jactionlint.uses {
        positions.jactionlint.push(index);
    }
}

fn claims_verifier(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::VerifyRustManifest | StepRole::VerifyJactionlint)
    ) || [
        crate::workflow::VERIFY_RESTORED_RUST_STEP,
        crate::workflow::VERIFY_RUST_MANIFEST_STEP,
        crate::workflow::VERIFY_JACTIONLINT_STEP,
    ]
    .iter()
    .any(|name| step.name.starts_with(name))
        || matches!(
            &step.kind,
            StepKind::Action { uses, .. }
                if uses.starts_with("./.github/actions/velnor-verify-")
        )
}

pub(super) fn rust_verifier_sequence_valid(
    job: &velnor_actions_contract::Job,
    positions: &VerifierPositions,
    prepare_at: Option<usize>,
) -> bool {
    let (Some(prepare_at), [restored_at], [installed_at]) = (
        prepare_at,
        positions.restored_rust.as_slice(),
        positions.installed_rust.as_slice(),
    ) else {
        return false;
    };
    let jactionlint_at = match positions.jactionlint.as_slice() {
        [] => None,
        [index] if prepare_at.checked_add(1) == Some(*index) => Some(*index),
        _ => return false,
    };
    let component_positions: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            (step.role == Some(StepRole::PrepareRustComponents)).then_some(index)
        })
        .collect();
    let first_component_after = jactionlint_at.unwrap_or(prepare_at);
    let components_follow_ordered_checks = component_positions
        .iter()
        .all(|components_at| *components_at > first_component_after);
    let last_setup_at = component_positions
        .last()
        .copied()
        .or(jactionlint_at)
        .unwrap_or(prepare_at);
    restored_at.checked_add(1) == Some(prepare_at)
        && components_follow_ordered_checks
        && last_setup_at.checked_add(1) == Some(*installed_at)
}

pub(super) fn jactionlint_verifier_sequence_valid(
    positions: &VerifierPositions,
    prepare_at: Option<usize>,
) -> bool {
    matches!(
        (prepare_at, positions.jactionlint.as_slice()),
        (Some(prepare_at), [verify_at]) if verify_at == &(prepare_at + 1)
    )
}

/// Bare-install diagnostic: zero specs means versions would come from
/// `mise.toml`, so silence would be a fail-open.
pub(super) fn bare_install(id: &str) -> String {
    format!(
        "bare_install_step:{id} installs zero specs (versions would come from mise.toml); emit explicit specs"
    )
}

/// Specs from a Prepare step's plain argv; a Prepare step always
/// installs, so a missing or unclassifiable vector blocks as a
/// generator bug.
pub(super) fn prepare_specs(
    id: &str,
    step: &Step,
    blocking: &mut Vec<String>,
) -> Option<Vec<String>> {
    let StepKind::Shell { run, .. } = &step.kind else {
        blocking.push(format!("unauditable_prepare_argv:{id}:not_a_shell_step"));
        return None;
    };
    let tokens: Vec<&str> = run.iter().map(String::as_str).collect();
    match classify_mise_vectors(&tokens) {
        MiseVectors::Install(specs) => Some(specs),
        MiseVectors::NotInstall => {
            blocking.push(format!("unauditable_prepare_argv:{id}:no_install"));
            None
        }
        MiseVectors::Unclassifiable(detail) => {
            blocking.push(format!("unauditable_prepare_argv:{id}:{detail}"));
            None
        }
    }
}

/// Audit one validator command's install contribution.
///
/// Isolated `exec` vectors contribute nothing; install specs resolve
/// through the validator pin only (a catalog spec here blocks as
/// misplaced instead of auditing through the wrong pin set), and an
/// unclassifiable `mise` vector blocks instead of passing silently.
pub(super) fn audit_validator_command(
    command: &ValidatorCommand,
    subjects: &mut Vec<InstallSubject>,
    blocking: &mut Vec<String>,
) {
    for argv in [&command.prepare_argv, &command.argv] {
        match validator_argv_specs(argv) {
            CommandInstalls::NotInstall => {}
            CommandInstalls::Unclassifiable(detail) => {
                blocking.push(format!(
                    "unauditable_validator_argv:{}:{detail}",
                    command.name
                ));
            }
            CommandInstalls::Specs(specs) => {
                if specs.is_empty() {
                    blocking.push(bare_install(&command.name));
                    continue;
                }
                for spec in specs {
                    match validator_subject(&spec) {
                        Some(subject) => subjects.push(subject),
                        None => blocking.push(format!("unauditable_install_spec:{spec}")),
                    }
                }
            }
        }
    }
}
