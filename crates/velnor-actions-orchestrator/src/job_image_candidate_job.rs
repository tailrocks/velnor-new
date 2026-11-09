// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner};
use velnor_actions_contract::{
    CheckoutHistory, GeneratorLock, JOB_IMAGE_CANDIDATE_ARTIFACT_PREFIX,
    JOB_IMAGE_CANDIDATE_JOB_ID, JOB_IMAGE_CANDIDATE_OUTPUTS, JOB_IMAGE_CANDIDATE_RUNNER,
    JOB_IMAGE_CANDIDATE_TASK_ID, Job, JobTimeout, Stack, Step, StepKind, WorkflowPolicy,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::verification_jobs::JobImageCandidateExecution;

use crate::OrchestratorError;
use crate::discover::Discovery;

pub(super) fn build(
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    published_lock: Option<&GeneratorLock>,
    helper_version: &str,
) -> Result<Option<(String, Job)>, OrchestratorError> {
    let proposal = discovery
        .proposals
        .iter()
        .find(|task| task.task_id == JOB_IMAGE_CANDIDATE_TASK_ID);
    let (execution, proposal) = match (discovery.job_image_candidate.as_ref(), proposal) {
        (None, None) => return Ok(None),
        (Some(execution), Some(proposal)) => (execution, proposal),
        _ => {
            return Err(OrchestratorError::Contract {
                problem: "job_image_candidate_discovery_mismatch".to_owned(),
            });
        }
    };
    velnor_actions_contract::validate_job_image_candidate_proposal(proposal, execution.inputs())?;
    let runner = CheckRunner {
        label: JOB_IMAGE_CANDIDATE_RUNNER.to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    };
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => crate::pins::consumer_acquire_for_runner(
            &runner,
            env!("CARGO_PKG_VERSION"),
            discovery.consumer_manifest_json.as_deref(),
        )?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            super::published_lock_acquire_for_runner(published_lock, &runner, helper_version)?
        }
    };
    let mut steps = vec![
        crate::workflow::wire_w1::checkout_step_for(CheckoutHistory::Full)?,
        acquire,
        crate::matrix_step::download_plan_step()?,
        crate::workflow_jobs::prepare_pinned_tools_step(
            catalog,
            vec![PinnedTool::Rust, PinnedTool::MrBoxington],
            true,
        )?,
        crate::workflow_jobs::prepare_rust_components_step(catalog)?,
        execute_step(execution, helper_version)?,
        upload_candidate(),
    ];
    let mut upload = crate::matrix_step::crate_upload_step(JOB_IMAGE_CANDIDATE_JOB_ID)?;
    upload.condition = Some("always()".to_owned());
    steps.push(upload);
    let job = Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: "Build Linux job image candidate".to_owned(),
        runs_on: JOB_IMAGE_CANDIDATE_RUNNER.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PRODUCT_COMPILER,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: Some(velnor_actions_workflow_renderer::not_affected_gate(
            JOB_IMAGE_CANDIDATE_JOB_ID,
        )),
        permissions: Some(crate::workflow_jobs::read_actions_permissions()),
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
        cache_access: velnor_actions_contract::CacheAccess::Read,
    };
    Ok(Some((JOB_IMAGE_CANDIDATE_JOB_ID.to_owned(), job)))
}

fn execute_step(
    execution: &JobImageCandidateExecution,
    helper_version: &str,
) -> Result<Step, OrchestratorError> {
    let toolchain_id =
        velnor_actions_contract::cachekey::toolchain_id(execution.binding_toolchain())?;
    let task_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        JOB_IMAGE_CANDIDATE_TASK_ID,
        execution.argv(),
        &toolchain_id,
    )?;
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(
        Stack::Native.id(),
        JOB_IMAGE_CANDIDATE_TASK_ID,
    )?;
    Ok(Step {
        name: "Execute planned job image candidate".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::TaskExecution {
            argv: execution.argv().to_vec(),
            env: execution.environment().clone(),
            task_id: JOB_IMAGE_CANDIDATE_TASK_ID.to_owned(),
            task_digest,
            toolchain_inputs: execution.binding_toolchain().clone(),
            matrix_key: velnor_actions_contract::matrix_key_for_id(&matrix_id)?,
            matrix_id,
            report_helper_version: helper_version.to_owned(),
            matrix_max_parallel: None,
        },
    })
}

fn upload_candidate() -> Step {
    let paths = JOB_IMAGE_CANDIDATE_OUTPUTS
        .iter()
        .map(|name| format!("${{{{ runner.temp }}}}/{name}"))
        .collect::<Vec<_>>()
        .join("\n");
    Step {
        name: "Retain planned job image candidate".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: velnor_actions_workflow_renderer::steps::UPLOAD_ARTIFACT_USES.to_owned(),
            with: BTreeMap::from([
                (
                    "name".to_owned(),
                    format!("{JOB_IMAGE_CANDIDATE_ARTIFACT_PREFIX}${{{{ github.run_attempt }}}}"),
                ),
                ("path".to_owned(), paths),
                ("if-no-files-found".to_owned(), "error".to_owned()),
                ("compression-level".to_owned(), "0".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}
