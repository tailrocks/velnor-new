// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{RootSlug, S256, SliceId};
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_LANE_CONDITION, STATE_PRODUCER_MAX_PARALLEL, state_producer_condition,
    state_producer_job_id, state_producer_lane_condition, state_producer_lane_slice,
    state_producer_matrix_output,
};
use velnor_actions_contract::{
    CacheAccess, Job, JobEnv, JobMatrix, JobTimeout, Step, StepKind, StepRole,
    canonical_json_bytes,
};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::declared_task_wrapper_digest;
use velnor_actions_workflow_renderer::state_seal::producer_steps::{
    state_payload_upload_step, state_producer_outputs,
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
use crate::matrix_step::LaneTools;

#[derive(Debug, Clone)]
pub(crate) struct ProducerSlice {
    pub(crate) slice: SliceId,
    pub(crate) root: String,
    pub(crate) lanes: Vec<Step>,
}

#[derive(Debug, Default)]
pub(crate) struct ProducerSlices(BTreeMap<SliceId, ProducerSlice>);

impl ProducerSlices {
    pub(super) fn add(
        &mut self,
        state: &mut crate::state_slices::SliceBuild,
        job_id: &str,
        lanes: super::lanes::ProducerLanes,
    ) -> Result<(), OrchestratorError> {
        let entry = self
            .0
            .entry(lanes.slice.clone())
            .or_insert_with(|| ProducerSlice {
                slice: lanes.slice.clone(),
                root: lanes.root.clone(),
                lanes: Vec::new(),
            });
        if entry.root != lanes.root {
            return Err(OrchestratorError::config(
                crate::config::CONFIG_REL,
                format!("stacks.rust.state_slices.{}", lanes.slice),
                "state_slice_spans_cargo_roots",
            ));
        }
        entry.lanes.extend(lanes.lanes);
        state.assign(job_id, &lanes.slice, &lanes.package_id);
        Ok(())
    }

    pub(super) fn into_slices(self) -> Vec<ProducerSlice> {
        self.0.into_values().collect()
    }
}

pub(crate) fn state_producer_jobs(
    input: &CrateJobInputs<'_>,
    lane: &LaneTools,
    slices: &[ProducerSlice],
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let mut roots: BTreeMap<&str, Vec<&ProducerSlice>> = BTreeMap::new();
    for slice in slices {
        roots.entry(slice.root.as_str()).or_default().push(slice);
    }
    roots
        .into_iter()
        .map(|(root, slices)| {
            let slug = RootSlug::from_root(root)?;
            Ok((
                state_producer_job_id(&slug),
                producer_job(input, lane, root, &slug, &slices, private, timeout)?,
            ))
        })
        .collect()
}

pub(crate) fn slice_views(
    producers: &[(String, Job)],
    slices: &[ProducerSlice],
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    slices
        .iter()
        .map(|slice| {
            let id = state_producer_job_id(&RootSlug::from_root(&slice.root)?);
            let (_, job) = producers
                .iter()
                .find(|(producer, _)| *producer == id)
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: format!("state_producer_missing:{}", slice.slice),
                })?;
            let mut view = job.clone();
            view.steps.retain(|step| {
                step.condition
                    .as_deref()
                    .and_then(state_producer_lane_slice)
                    .is_none_or(|owner| owner == slice.slice)
            });
            Ok((slice.slice.to_string(), view))
        })
        .collect()
}

pub(crate) fn recipe_digests(
    producers: &[(String, Job)],
    helper_version: &str,
    generator_version: &str,
) -> Result<BTreeMap<String, S256>, OrchestratorError> {
    let wrapper = declared_task_wrapper_digest(helper_version, generator_version)?;
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
    root: &str,
    slug: &RootSlug,
    slices: &[&ProducerSlice],
    private: Option<&PrivateWiring>,
    timeout: JobTimeout,
) -> Result<Job, OrchestratorError> {
    let catalog = input.catalog;
    crate::source_prep::validate_root(root)?;
    let id = state_producer_job_id(slug);
    let mut steps = vec![
        velnor_actions_workflow_renderer::state_seal::tip::stale_tip_stops_job_step()?,
        crate::workflow::wire_w1::checkout_step()?,
    ];
    steps.extend(input.acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(crate::workflow::verify_restored_rust_step(catalog));
    steps.push(crate::matrix_step::prepare_crate_tools_step(catalog, lane)?);
    steps.extend(crate::matrix_step::verify_validators_step(catalog, lane));
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    steps.extend(crate::source_cache::restore_step_for_crate(
        input.fetch_roots,
        true,
    )?);
    let working_dir = (!root.is_empty()).then_some(root);
    steps.extend(crate::mbx_preflight::steps_for_catalog(
        catalog,
        MbxTransport::ProducerMatrix,
        working_dir,
    )?);
    let roots = vec![root.to_owned()];
    if let Some(private) = private {
        steps.extend(private.steps_for_roots(&roots)?);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog,
        &roots,
        slices.iter().any(|slice| lanes_use_all_features(&slice.lanes)),
    )?);
    let toolchain = catalog.version(PinnedTool::Rust).to_owned();
    let toolchain_env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    steps.push(state_skeleton_step(
        &toolchain,
        &root_argument(root),
        toolchain_env.clone(),
    )?);
    steps.extend(slices.iter().flat_map(|slice| lane_steps(slice)));
    steps.push(state_skeleton_restore_step()?);
    steps.extend(slices.iter().flat_map(|slice| lane_steps(slice)));
    steps.push(velnor_actions_workflow_renderer::mbx_stats::mbx_stats_step(
        &id,
        &crate::source_prep::workspace_crate_names(&input.discovery.workspaces, root),
    )?);
    let mut mark = mbx_producer_mark_step()?;
    mark.condition = Some(STATE_PRODUCER_LANE_CONDITION.to_owned());
    steps.push(mark);
    steps.push(mbx_bundle_publish_bounded(
        &toolchain,
        toolchain_env,
        StateCeilings::SLICE,
    )?);
    steps.push(state_payload_upload_step()?);
    Ok(Job {
        services: Vec::new(),
        outputs: state_producer_outputs(slices.iter().map(|slice| &slice.slice)),
        display_name: display_name(root),
        runs_on: input.label.to_owned(),
        check_runner: None,
        timeout_minutes: timeout,
        needs: vec![velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned()],
        condition: Some(state_producer_condition(slug)),
        permissions: None,
        environment: None,
        env: JobEnv::Unset,
        matrix: Some(JobMatrix {
            include_output: state_producer_matrix_output(slug),
            max_parallel: STATE_PRODUCER_MAX_PARALLEL,
            legs: u32::try_from(slices.len()).map_err(|_| OrchestratorError::Contract {
                problem: format!("state_producer_legs_overflow:{id}"),
            })?,
        }),
        steps,
        cache_access: CacheAccess::Read,
    })
}

fn root_argument(root: &str) -> String {
    if root.is_empty() {
        ".".to_owned()
    } else {
        root.to_owned()
    }
}

fn lane_steps(slice: &ProducerSlice) -> impl Iterator<Item = Step> + '_ {
    let condition = state_producer_lane_condition(&slice.slice);
    slice.lanes.iter().map(move |lane| {
        let mut step = lane.clone();
        step.condition = Some(condition.clone());
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
        Ok(crate::matrix_step::obligation_step(
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
        let env = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)?;
        let steps = vec![
            state_skeleton_step(&toolchain, ".", env)?,
            lane(argv)?,
        ];
        let job = Job {
            services: Vec::new(),
            outputs: BTreeMap::new(),
            display_name: id.to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::new(30)?,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            env: JobEnv::Unset,
            matrix: None,
            steps,
            cache_access: CacheAccess::Read,
        };
        Ok((id.to_owned(), job))
    }

    #[test]
    fn recipe_moves_with_the_task_wrapper_and_only_with_it() -> Result<(), Box<dyn Error>> {
        let jobs = [producer("state-producer-x", &["cargo", "clippy"])?];
        let old = recipe_digests(&jobs, HELPER_VERSION, "0.1.73")?;
        let again = recipe_digests(&jobs, HELPER_VERSION, "0.1.73")?;
        let new = recipe_digests(&jobs, HELPER_VERSION, "0.1.74")?;
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
        let before = recipe_digests(&before, HELPER_VERSION, "0.1.73")?;
        let after = recipe_digests(&after, HELPER_VERSION, "0.1.73")?;
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
