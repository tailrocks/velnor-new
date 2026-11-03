// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::RootSlug;
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_LANE_CONDITION, state_producer_condition, state_producer_job_id,
};
use velnor_actions_contract::{CacheAccess, Job, JobEnv, JobTimeout, Step, StepKind};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::state_producer_steps::{
    state_payload_upload_step, state_produced_step, state_producer_outputs,
};
use velnor_actions_workflow_renderer::state_skeleton::{
    state_skeleton_restore_step, state_skeleton_step,
};
use velnor_actions_workflow_renderer::steps::{
    MbxTransport, StateCeilings, mbx_bundle_publish_bounded, mbx_producer_mark_step,
};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;

#[derive(Debug, Clone)]
pub(crate) struct ProducerRoot {
    pub(crate) root: String,
    pub(crate) nextest: bool,
    pub(crate) lanes: Vec<Step>,
}

pub(crate) fn state_producer_jobs(
    input: &CrateJobInputs<'_>,
    roots: &[ProducerRoot],
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    roots
        .iter()
        .map(|root| {
            let slug = RootSlug::from_root(&root.root)?;
            Ok((
                state_producer_job_id(&slug),
                producer_job(input, root, &slug, private, timeout)?,
            ))
        })
        .collect()
}

pub(crate) fn producer_job_ids(roots: &[String]) -> Result<Vec<String>, OrchestratorError> {
    roots
        .iter()
        .map(|root| Ok(state_producer_job_id(&RootSlug::from_root(root)?)))
        .collect()
}

fn producer_job(
    input: &CrateJobInputs<'_>,
    producer: &ProducerRoot,
    slug: &RootSlug,
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Job, OrchestratorError> {
    let catalog = input.catalog;
    crate::source_prep::validate_root(&producer.root)?;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(input.acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(crate::workflow::verify_restored_rust_step(catalog));
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        true,
        producer.nextest,
        false,
        false,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    steps.extend(crate::source_cache::restore_step_for_crate(
        input.fetch_roots,
        true,
    )?);
    let working_dir = (!producer.root.is_empty()).then_some(producer.root.as_str());
    steps.extend(crate::mbx_preflight::steps_for_catalog(
        catalog,
        MbxTransport::SeedRead,
        working_dir,
    )?);
    let roots = vec![producer.root.clone()];
    if let Some(private) = private {
        steps.extend(private.steps_for_roots(&roots)?);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog,
        &roots,
        lanes_use_all_features(&producer.lanes),
    )?);
    let toolchain = catalog.version(PinnedTool::Rust).to_owned();
    let toolchain_env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    steps.push(state_skeleton_step(
        &toolchain,
        &producer.root_argument(),
        toolchain_env.clone(),
    )?);
    steps.extend(lane_steps(&producer.lanes));
    steps.push(state_skeleton_restore_step()?);
    steps.extend(lane_steps(&producer.lanes));
    let mut mark = mbx_producer_mark_step()?;
    mark.condition = Some(STATE_PRODUCER_LANE_CONDITION.to_owned());
    steps.push(mark);
    steps.push(mbx_bundle_publish_bounded(
        &toolchain,
        toolchain_env,
        StateCeilings::V3,
    )?);
    steps.push(state_payload_upload_step(slug)?);
    steps.push(state_produced_step()?);
    Ok(Job {
        services: Vec::new(),
        outputs: state_producer_outputs(),
        display_name: display_name(&producer.root),
        runs_on: input.label.to_owned(),
        check_runner: None,
        timeout_minutes: timeout,
        needs: vec![velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned()],
        condition: Some(state_producer_condition(slug.as_str())),
        permissions: None,
        environment: None,
        env: JobEnv::Unset,
        steps,
        cache_access: CacheAccess::Read,
    })
}

impl ProducerRoot {
    fn root_argument(&self) -> String {
        if self.root.is_empty() {
            ".".to_owned()
        } else {
            self.root.clone()
        }
    }
}

fn lane_steps(lanes: &[Step]) -> impl Iterator<Item = Step> + '_ {
    lanes.iter().map(|lane| {
        let mut step = lane.clone();
        step.condition = Some(STATE_PRODUCER_LANE_CONDITION.to_owned());
        step
    })
}

fn lanes_use_all_features(lanes: &[Step]) -> bool {
    lanes.iter().any(|lane| {
        matches!(&lane.kind, StepKind::TaskExecution { argv, .. } if argv.iter().any(|arg| arg == "--all-features"))
    })
}

fn display_name(root: &str) -> String {
    if root.is_empty() {
        "State producer".to_owned()
    } else {
        format!("State producer / {root}")
    }
}
