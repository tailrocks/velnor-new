// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JOB_IMAGE_CANDIDATE_JOB_ID, JOB_IMAGE_CANDIDATE_RUNNER,
    JOB_IMAGE_CANDIDATE_TASK_ID, JOB_IMAGE_CANDIDATE_TASK_KIND, JOB_IMAGE_CANDIDATE_TASK_SCHEMA,
    JOB_IMAGE_CANDIDATE_UNIT_ID, MatrixEntry, ObligationDecision, PlanObligation, PlannedPlatform,
    StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput, VcsInputs,
    canonical_json_bytes, digest_b3, input_digest, selects_affected_work,
};
use velnor_actions_workflow_renderer::verification_jobs::JobImageCandidateExecution;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::safe_read::{RepoBytes, read_repo_bytes};

use super::GroupInputs;

pub(super) fn derive(
    inputs: &GroupInputs<'_>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let execution = inputs
        .discovery
        .job_image_candidate
        .as_ref()
        .ok_or_else(|| internal("job_image_candidate_not_configured"))?;
    velnor_actions_contract::validate_job_image_candidate_proposal(inputs.task, execution.inputs())
        .map_err(internal_contract)?;
    let task = inputs.task;
    let platform = PlannedPlatform::new(JOB_IMAGE_CANDIDATE_RUNNER, &task.identity.target)
        .map_err(internal_contract)?;
    let identity = candidate_identity(inputs, execution, &platform)?;
    let closure = digest_b3(&canonical_json_bytes(&identity.inputs).map_err(internal_contract)?);
    let binding = velnor_actions_contract::cachekey::toolchain_id(execution.binding_toolchain())
        .map_err(internal_contract)?;
    let digest = input_digest(&identity).map_err(internal_contract)?;
    let task_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        JOB_IMAGE_CANDIDATE_TASK_ID,
        execution.argv(),
        &binding,
    )
    .map_err(internal_contract)?;
    let run = selected(inputs);
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: if run {
            ObligationDecision::Execute
        } else {
            ObligationDecision::NotAffected
        },
        reason: if run {
            "affected_by_change"
        } else {
            "not_affected"
        }
        .to_owned(),
        task_digest: task_digest.clone(),
        input_digest: digest.clone(),
        closure_digest: closure,
        job_id: JOB_IMAGE_CANDIDATE_JOB_ID.to_owned(),
    };
    let mut entry = MatrixEntry::derive(
        &task.stack_id,
        &task.task_id,
        &velnor_actions_workflow_renderer::join_argv_for_run(execution.argv())
            .map_err(|error| internal(&error.to_string()))?,
        &task_digest,
        serde_json::json!({"unit_id": JOB_IMAGE_CANDIDATE_UNIT_ID}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                JOB_IMAGE_CANDIDATE_TASK_KIND.to_owned(),
                ExecuteTaskRef::Single(task.task_id.clone()),
            )]),
        },
        &digest,
        inputs.run_key,
        JOB_IMAGE_CANDIDATE_JOB_ID,
        platform,
    )
    .map_err(internal_contract)?;
    entry.declared_outputs.clone_from(&task.outputs);
    Ok((obligation, vec![entry]))
}

fn candidate_identity(
    inputs: &GroupInputs<'_>,
    execution: &JobImageCandidateExecution,
    platform: &PlannedPlatform,
) -> Result<TaskIdentity, OrchestratorError> {
    let task = inputs.task;
    let mut files = Vec::with_capacity(task.reads.len());
    for path in &task.reads {
        let RepoBytes::Bytes(bytes) = read_repo_bytes(inputs.root, path, 16 * 1024 * 1024)? else {
            return Err(internal("job_image_candidate_input_missing"));
        };
        files.push(TaskInput {
            path: path.clone(),
            digest: digest_b3(&bytes),
        });
    }
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(execution.actual_toolchain())
        .map_err(internal_contract)?;
    Ok(TaskIdentity {
        schema_version: 1,
        stack_id: task.stack_id.clone(),
        project_root: task.identity.project_root.clone(),
        component_id: task.component_id.clone(),
        task_kind: task.task_kind.clone(),
        task_id: task.task_id.clone(),
        argv: execution.argv().to_vec(),
        working_dir: ".".to_owned(),
        configuration: TaskConfiguration {
            target: task.identity.target.clone(),
            profile: task.configuration.clone(),
            features: task.identity.features.clone(),
            flags: task.identity.flags.clone(),
            task_contract: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
            compile_driver: task.identity.compile_driver.clone(),
            test_runner: task.identity.test_runner.clone(),
        },
        inputs: files,
        dependencies: task.depends_on.clone(),
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: toolchain,
        platform_id: platform.platform_id.clone(),
        environment: execution.environment().clone(),
        output_contract: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
        generator: TaskGenerator {
            version: inputs.wire.generator.version.clone(),
            target: inputs.wire.generator.target.clone(),
        },
        stack_extension: StackExtension {
            schema: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
            data: serde_json::to_value(execution.inputs())
                .map_err(|_| internal("job_image_candidate_identity"))?,
        },
    })
}

fn selected(inputs: &GroupInputs<'_>) -> bool {
    if !selects_affected_work(inputs.wire.event) {
        return true;
    }
    let (Some(changed), Some(added), Some(_)) = (
        &inputs.change_evidence.paths,
        &inputs.change_evidence.added_paths,
        inputs.affected_packages,
    ) else {
        return true;
    };
    let workflow =
        velnor_actions_workflow_renderer::schema2::StandaloneWorkflow::JobImageCandidate.path();
    if changed.contains(workflow) || inputs.task.reads.iter().any(|path| changed.contains(path)) {
        return true;
    }
    let owners: Vec<_> = inputs
        .discovery
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.record.packages)
        .filter(|package| package.in_workspace && !package.external)
        .filter(|package| {
            let directory = crate::select_affected::manifest_dir(&package.manifest);
            !directory.is_empty()
                && inputs
                    .task
                    .reads
                    .iter()
                    .any(|path| path.starts_with(&format!("{directory}/")))
        })
        .collect();
    owners.is_empty()
        || owners.iter().any(|package| {
            let directory = crate::select_affected::manifest_dir(&package.manifest);
            added
                .iter()
                .any(|path| path.starts_with(&format!("{directory}/")))
        })
}
