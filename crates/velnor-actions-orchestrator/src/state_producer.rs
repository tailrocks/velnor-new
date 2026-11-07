// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{RootSlug, S256};
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_LANE_CONDITION, state_producer_condition, state_producer_job_id,
};
use velnor_actions_contract::{
    CacheAccess, Job, JobEnv, JobTimeout, Step, StepKind, StepRole, canonical_json_bytes,
};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::declared_task_wrapper_digest;
use velnor_actions_workflow_renderer::state_seal::producer_steps::{
    state_payload_upload_step, state_produced_step, state_producer_outputs,
};
use velnor_actions_workflow_renderer::state_seal::skeleton::{
    state_skeleton_restore_step, state_skeleton_step,
};
use velnor_actions_workflow_renderer::steps::{
    MbxTransport, StateCeilings, mbx_bundle_publish_bounded, mbx_producer_mark_step,
};

use super::CrateJobInputs;
use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::planner::matrix_step::LaneTools;

#[derive(Debug, Clone)]
pub(crate) struct ProducerRoot {
    pub(crate) root: String,
    pub(crate) lanes: Vec<Step>,
}

pub(crate) fn state_producer_jobs(
    input: &CrateJobInputs<'_>,
    lane: &LaneTools,
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
                producer_job(input, lane, root, &slug, private, timeout)?,
            ))
        })
        .collect()
}

pub(crate) fn recipe_digests(
    producers: &[(String, Job)],
    helper_version: &str,
    generator_version: &str,
    preview: Option<&velnor_actions_contract::source_preview::SourcePreviewLock>,
) -> Result<BTreeMap<String, S256>, OrchestratorError> {
    let wrapper = if let Some(preview) = preview {
        velnor_actions_workflow_renderer::preview_task_wrapper_digest(preview, generator_version)?
    } else {
        declared_task_wrapper_digest(helper_version, generator_version)?
    };
    producers
        .iter()
        .map(|(id, job)| {
            let start = job
                .steps
                .iter()
                .position(|step| step.role == Some(StepRole::StateSkeleton))
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: format!("state_producer_skeleton_missing:{id}"),
                })?;
            Ok((
                id.clone(),
                S256::of(&canonical_json_bytes(&(
                    &job.steps[start..],
                    wrapper.to_string(),
                ))?),
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
    lane: &LaneTools,
    producer: &ProducerRoot,
    slug: &RootSlug,
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Job, OrchestratorError> {
    let catalog = input.catalog;
    crate::planner::source_prep::validate_root(&producer.root)?;
    let mut steps = vec![
        velnor_actions_workflow_renderer::state_seal::tip::stale_tip_stops_job_step()?,
        crate::workflow::wire_w1::checkout_step()?,
    ];
    steps.extend(input.acquire.cloned());
    steps.push(crate::planner::matrix_step::download_plan_step()?);
    steps.push(crate::workflow::verify_restored_rust_step(catalog));
    steps.push(crate::planner::matrix_step::prepare_crate_tools_step(
        catalog, lane,
    )?);
    steps.extend(crate::planner::matrix_step::verify_validators_step(
        catalog, lane,
    ));
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    steps.extend(crate::planner::source_cache::restore_step_for_crate(
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
    steps.extend(crate::planner::source_prep::fetch_steps_for_crate(
        catalog,
        &roots,
        lanes_use_all_features(&producer.lanes),
    )?);
    let toolchain = catalog.version(PinnedTool::Rust).to_owned();
    let toolchain_env =
        crate::planner::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    steps.push(state_skeleton_step(
        &toolchain,
        &producer.root_argument(),
        toolchain_env.clone(),
    )?);
    steps.extend(lane_steps(&producer.lanes));
    steps.push(state_skeleton_restore_step()?);
    steps.extend(lane_steps(&producer.lanes));
    steps.push(velnor_actions_workflow_renderer::mbx_stats::mbx_stats_step(
        &state_producer_job_id(slug),
        &crate::planner::source_prep::workspace_crate_names(
            &input.discovery.workspaces,
            &producer.root,
        ),
    )?);
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

#[cfg(test)]
mod tests {
    use std::error::Error;

    use velnor_actions_contract::CrateObligation;
    use velnor_actions_contract::cachekey::ToolchainInputs;
    use velnor_actions_mise::ToolCatalog;

    use super::*;

    const HELPER_VERSION: &str = "0.1.73";

    fn lane(argv: &[&str]) -> Result<Step, Box<dyn Error>> {
        let obligation = CrateObligation {
            task_id: "stack/rust/crates/demo/clippy/default".to_owned(),
            kind: "clippy".to_owned(),
            step_name: "Clippy".to_owned(),
            gated_by: Vec::new(),
            matrix_key: "clippy".to_owned(),
            task_digest: "b3-0000000000000000000000000000000000000000000000000000000000000000"
                .to_owned(),
            toolchain_inputs: ToolchainInputs {
                tools: Vec::new(),
                components: Vec::new(),
                compile_driver: "cargo".to_owned(),
                test_runner: "nextest".to_owned(),
            },
            run: argv.iter().map(|arg| (*arg).to_owned()).collect(),
        };
        Ok(crate::planner::matrix_step::obligation_step(
            &obligation,
            &ToolCatalog::pinned(),
            &[],
            None,
            HELPER_VERSION,
            None,
        )?)
    }

    fn producer(id: &str, argv: &[&str]) -> Result<(String, Job), Box<dyn Error>> {
        let catalog = ToolCatalog::pinned();
        let toolchain = catalog.version(PinnedTool::Rust).to_owned();
        let env = crate::planner::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)?;
        let steps = vec![state_skeleton_step(&toolchain, ".", env)?, lane(argv)?];
        let job = Job {
            services: Vec::new(),
            outputs: BTreeMap::new(),
            display_name: id.to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::new(30)?,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            env: JobEnv::Unset,
            steps,
            cache_access: CacheAccess::Read,
        };
        Ok((id.to_owned(), job))
    }

    #[test]
    fn recipe_moves_with_the_task_wrapper_and_only_with_it() -> Result<(), Box<dyn Error>> {
        let jobs = [producer("state-producer-x", &["cargo", "clippy"])?];
        let old = recipe_digests(&jobs, HELPER_VERSION, "0.1.73", None)?;
        let again = recipe_digests(&jobs, HELPER_VERSION, "0.1.73", None)?;
        let new = recipe_digests(&jobs, HELPER_VERSION, "0.1.74", None)?;
        assert_eq!(old, again, "the recipe is not deterministic");
        assert_ne!(
            old, new,
            "a new generator version changed the wrapper file but not the recipe"
        );
        Ok(())
    }

    #[test]
    fn recipe_of_one_producer_ignores_another_producers_lanes() -> Result<(), Box<dyn Error>> {
        let before = [
            producer("state-producer-x", &["cargo", "clippy"])?,
            producer("state-producer-y", &["cargo", "clippy"])?,
        ];
        let after = [
            producer("state-producer-x", &["cargo", "clippy"])?,
            producer("state-producer-y", &["cargo", "clippy", "--all-features"])?,
        ];
        let before = recipe_digests(&before, HELPER_VERSION, "0.1.73", None)?;
        let after = recipe_digests(&after, HELPER_VERSION, "0.1.73", None)?;
        assert_eq!(
            before["state-producer-x"], after["state-producer-x"],
            "a lane change in Y moved X"
        );
        assert_ne!(
            before["state-producer-y"], after["state-producer-y"],
            "a lane change in Y left Y unchanged"
        );
        Ok(())
    }
}
