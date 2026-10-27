// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_mise::toolfiles::lockfile::{InstallSubject, subject_for_install_spec};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::workflow::verifier_actions::VerifierActionSet;

use super::{
    CommandInstalls, InstallAuditAuthority, MiseVectors, validator_argv_specs, validator_subject,
    vectors::classify_mise_vectors,
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
    policy: WorkflowPolicy,
    blocking: &mut Vec<String>,
) -> VerifierPositions {
    let mut positions = VerifierPositions::default();
    let allow_inline_plan_verifiers = policy == WorkflowPolicy::VelnorRepositoryV1
        && id == velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
    for (index, step) in job.steps.iter().enumerate() {
        let action = match &step.kind {
            StepKind::Action { uses, .. } => verifiers.by_uses(uses),
            StepKind::Shell { .. } if allow_inline_plan_verifiers => {
                verifiers.by_canonical_inline_call(step)
            }
            StepKind::Shell { .. }
            | StepKind::TaskExecution { .. }
            | StepKind::Internal { .. }
            | StepKind::NativeCheckExecution { .. } => None,
        };
        if let Some(action) = action {
            if matches!(&step.kind, StepKind::Action { .. })
                && !VerifierActionSet::canonical_call(step, action)
            {
                blocking.push(format!("noncanonical_verifier_call:{id}:{}", action.uses));
                continue;
            }
            match action.uses {
                uses if uses == verifiers.restored_rust.uses => {
                    positions.restored_rust.push(index);
                }
                uses if uses == verifiers.installed_rust.uses => {
                    positions.installed_rust.push(index);
                }
                uses if uses == verifiers.jactionlint.uses => {
                    positions.jactionlint.push(index);
                }
                _ => unreachable!("lookup returns only registered verifier actions"),
            }
        } else if matches!(
            step.role,
            Some(StepRole::VerifyRustManifest | StepRole::VerifyJactionlint)
        ) || matches!(
            step.name.as_str(),
            crate::workflow::VERIFY_RESTORED_RUST_STEP
                | crate::workflow::VERIFY_RUST_MANIFEST_STEP
                | crate::workflow::VERIFY_JACTIONLINT_STEP
        ) {
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
    let components_at = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PrepareRustComponents));
    restored_at.checked_add(1) == Some(prepare_at)
        && *installed_at > prepare_at
        && components_at.is_none_or(|at| *installed_at > at)
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

pub(super) fn bare_install(id: &str) -> String {
    format!(
        "bare_install_step:{id} installs zero specs (versions would come from mise.toml); emit explicit specs"
    )
}

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

pub(super) fn published_helper_catalog(
    authority: InstallAuditAuthority<'_>,
    catalog: &ToolCatalog,
    blocking: &mut Vec<String>,
) -> Option<ToolCatalog> {
    if authority.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return None;
    }
    let Some(lock) = authority.published_generator_lock else {
        blocking.push("required_verdict_requires_published_generator_lock".to_owned());
        return None;
    };
    let Some(approved) = authority.approved_release else {
        blocking.push("controller_approvals:missing_approval".to_owned());
        return None;
    };
    let helper = velnor_actions_mise::catalog::lock::catalog_for_approved_generator(
        lock,
        catalog,
        &approved.as_authority(),
    );
    match helper {
        Ok(helper) => Some(helper),
        Err(error) => {
            blocking.push(error.to_string());
            None
        }
    }
}

pub(super) fn install_subject(
    job_id: &str,
    role: Option<StepRole>,
    spec: &str,
    catalog: &ToolCatalog,
    helper_catalog: Option<&ToolCatalog>,
) -> Option<InstallSubject> {
    subject_for_install_spec(spec, catalog).or_else(|| {
        if role != Some(StepRole::PreparePinnedTools)
            || !matches!(
                job_id,
                velnor_actions_workflow_renderer::render::PLAN_JOB_ID
                    | velnor_actions_workflow_renderer::render::FINAL_JOB_ID
            )
        {
            return None;
        }
        let helper = helper_catalog?;
        if spec != helper.tool_spec(PinnedTool::Gh) {
            return None;
        }
        subject_for_install_spec(spec, helper)
    })
}
