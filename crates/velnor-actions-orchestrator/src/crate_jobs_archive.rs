//! Source-bound archive producers and shard routes for crate jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{CrateJob, CrateObligation, Job, Step};
use velnor_actions_mise::{NextestDriver, NextestPartition, ToolCatalog};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::discover::Discovery;

const ARCHIVE_DOWNLOAD_PATH: &str = "target/nextest";
const ARCHIVE_PATH: &str = "target/nextest/tests.tar.zst";

#[path = "crate_jobs_archive_plan.rs"]
mod plan;
#[cfg(test)]
pub(super) use plan::build_archive_plan;
pub(super) use plan::build_crate_jobs_inner;
pub(crate) use plan::build_for_workflow;

#[derive(Clone)]
struct ArchiveRoute {
    producer_id: String,
    artifact_name: String,
    driver: NextestDriver,
    partition: NextestPartition,
    profile: String,
    run_ignored: Option<String>,
    filter: Option<String>,
}

struct Producer {
    job: Job,
    driver: RenderDriver,
}

#[derive(Default)]
pub(super) struct ArchivePlan {
    routes: BTreeMap<String, ArchiveRoute>,
    producers: BTreeMap<String, Producer>,
}

impl ArchivePlan {
    pub(super) fn empty() -> Self {
        Self::default()
    }

    pub(super) fn download_steps(
        &self,
        obligations: &[CrateObligation],
    ) -> Result<Vec<Step>, OrchestratorError> {
        let mut seen = BTreeSet::new();
        let mut steps = Vec::new();
        for obligation in obligations {
            let Some(route) = self.routes.get(&obligation.task_id) else {
                continue;
            };
            if !seen.insert(route.artifact_name.as_str()) {
                continue;
            }
            let mut step = velnor_actions_workflow_renderer::steps::download_artifact_step(
                &route.artifact_name,
                ARCHIVE_DOWNLOAD_PATH,
            )
            .map_err(OrchestratorError::from)?;
            step.name = "Download Nextest archive".to_owned();
            steps.push(step);
        }
        Ok(steps)
    }

    pub(super) fn consumer_steps(
        &self,
        model: &CrateJob,
        catalog: &ToolCatalog,
        max_parallel_jobs: u32,
        use_opentofu: bool,
    ) -> Result<Vec<Step>, OrchestratorError> {
        let mut steps = self.download_steps(&model.obligations)?;
        for (index, obligation) in model.obligations.iter().enumerate() {
            let downstream = model.obligations[index + 1..]
                .iter()
                .map(|later| later.task_id.clone())
                .collect::<Vec<_>>();
            let cap = (index == 0 && use_opentofu).then_some(max_parallel_jobs);
            let routed = self.routed_obligation(obligation, catalog)?;
            steps.push(crate::matrix_step::obligation_step(
                routed.as_ref().unwrap_or(obligation),
                catalog,
                &downstream,
                cap,
            )?);
        }
        Ok(steps)
    }

    pub(super) fn dependencies(&self, obligations: &[CrateObligation]) -> Vec<String> {
        let mut producers = BTreeSet::new();
        for obligation in obligations {
            if let Some(route) = self.routes.get(&obligation.task_id) {
                producers.insert(route.producer_id.clone());
            }
        }
        std::iter::once(PLAN_JOB_ID.to_owned())
            .chain(producers)
            .collect()
    }

    pub(super) fn routed_obligation(
        &self,
        obligation: &CrateObligation,
        catalog: &ToolCatalog,
    ) -> Result<Option<velnor_actions_contract::CrateObligation>, OrchestratorError> {
        let Some(route) = self.routes.get(&obligation.task_id) else {
            return Ok(None);
        };
        let mut routed = obligation.clone();
        routed.run = routed_nextest_argv(obligation, route, catalog)?;
        Ok(Some(routed))
    }

    pub(super) fn take_jobs(&mut self) -> Vec<(String, Job, RenderDriver)> {
        std::mem::take(&mut self.producers)
            .into_iter()
            .map(|(id, producer)| (id, producer.job, producer.driver))
            .collect()
    }
}

fn routed_nextest_argv(
    obligation: &CrateObligation,
    route: &ArchiveRoute,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    crate::vectors::nextest_shard_argv(
        route.driver,
        route.partition,
        &route.profile,
        &obligation.matrix_key,
        route.run_ignored.as_deref(),
        route.filter.as_deref(),
        catalog,
    )
}

#[cfg(test)]
#[path = "../tests/impl_orch_nextest_archive.rs"]
mod tests;
