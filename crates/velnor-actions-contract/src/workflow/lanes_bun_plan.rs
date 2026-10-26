// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::ir::{Job, StepKind};
use super::{HOSTED_SUFFIX, LaneClass, Placement, SCALE_SUFFIX};
use crate::config::BUN_JOB_PREFIX;
use crate::errors::ContractError;

#[derive(Debug, Clone)]
pub struct BunLanePlan {
    routes: BTreeMap<String, JobRoute>,
    id_map: BTreeMap<String, Vec<String>>,
    output_to_logical: BTreeMap<String, String>,
    asset_producers: BTreeMap<String, String>,
    scale_token: Option<String>,
}

#[derive(Debug, Clone)]
struct JobRoute {
    class: LaneClass,
    placement: Placement,
    outputs: Vec<LaneOutput>,
}

#[derive(Debug, Clone)]
struct LaneOutput {
    id: String,
    lane: PhysicalLane,
    paired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhysicalLane {
    Hosted,
    Scale,
}

impl BunLanePlan {
    pub(super) fn from_placements(
        placements: &BTreeMap<String, (LaneClass, Placement)>,
        id_map: &BTreeMap<String, Vec<String>>,
        scale_token: Option<String>,
        asset_producers: BTreeMap<String, String>,
    ) -> Self {
        let mut routes = BTreeMap::new();
        let mut output_to_logical = BTreeMap::new();
        for (logical_id, (class, placement)) in placements {
            let outputs = lane_outputs(logical_id, *placement);
            for output in &outputs {
                output_to_logical.insert(output.id.clone(), logical_id.clone());
            }
            routes.insert(
                logical_id.clone(),
                JobRoute {
                    class: *class,
                    placement: *placement,
                    outputs,
                },
            );
        }
        Self {
            routes,
            id_map: id_map.clone(),
            output_to_logical,
            asset_producers,
            scale_token,
        }
    }

    #[must_use]
    pub fn logical_id_for_output(&self, output_id: &str) -> Option<&str> {
        self.output_to_logical.get(output_id).map(String::as_str)
    }

    pub fn output_ids(&self, logical_id: &str) -> Result<Vec<String>, ContractError> {
        let route = self.route(logical_id)?;
        Ok(route
            .outputs
            .iter()
            .map(|output| output.id.clone())
            .collect())
    }

    pub fn project(
        &self,
        logical_id: &str,
        canonical_job: &Job,
    ) -> Result<Vec<(String, Job)>, ContractError> {
        let route = self.route(logical_id)?;
        let mut projected = super::emit_job_variants(
            logical_id,
            canonical_job,
            self.scale_token.as_deref(),
            route.class,
            route.placement,
            &self.id_map,
        )?;
        for (output_id, job) in &mut projected {
            let output = route
                .outputs
                .iter()
                .find(|output| output.id == *output_id)
                .ok_or_else(|| plan_error("output_route_missing", logical_id))?;
            let identities = self.output_identities(logical_id, output, canonical_job)?;
            self.project_needs(canonical_job, &identities, job)?;
            project_condition(canonical_job, &identities, job);
            project_steps(&identities, output, job);
        }
        Ok(projected)
    }

    fn route(&self, logical_id: &str) -> Result<&JobRoute, ContractError> {
        self.routes
            .get(logical_id)
            .filter(|_| logical_id.starts_with(BUN_JOB_PREFIX))
            .ok_or_else(|| plan_error("unknown_logical_id", logical_id))
    }

    fn output_identities(
        &self,
        logical_id: &str,
        output: &LaneOutput,
        canonical_job: &Job,
    ) -> Result<BTreeMap<String, String>, ContractError> {
        let mut identities = BTreeMap::from([(logical_id.to_owned(), output.id.clone())]);
        for dependency in canonical_job
            .needs
            .iter()
            .filter(|dependency| dependency.starts_with(BUN_JOB_PREFIX))
        {
            let producer = self.route(dependency)?;
            let target = if producer.outputs.len() == 1 {
                &producer.outputs[0]
            } else {
                producer
                    .outputs
                    .iter()
                    .find(|target| target.lane == output.lane)
                    .ok_or_else(|| plan_error("producer_lane_missing", dependency))?
            };
            identities.insert(dependency.to_owned(), target.id.clone());
        }
        for (asset_id, producer_id) in &self.asset_producers {
            let Some(producer_output_id) = identities.get(producer_id) else {
                continue;
            };
            let producer = self.route(producer_id)?;
            let producer_output = producer
                .outputs
                .iter()
                .find(|candidate| candidate.id == *producer_output_id)
                .ok_or_else(|| plan_error("asset_producer_output_missing", producer_id))?;
            let asset_output_id = if producer_output.paired {
                let suffix = match producer_output.lane {
                    PhysicalLane::Hosted => HOSTED_SUFFIX,
                    PhysicalLane::Scale => SCALE_SUFFIX,
                };
                format!("{asset_id}{suffix}")
            } else {
                asset_id.clone()
            };
            if let Some(existing) = identities.insert(asset_id.clone(), asset_output_id.clone())
                && existing != asset_output_id
            {
                return Err(plan_error("asset_identity_collision", asset_id));
            }
        }
        Ok(identities)
    }

    fn project_needs(
        &self,
        canonical_job: &Job,
        identities: &BTreeMap<String, String>,
        projected_job: &mut Job,
    ) -> Result<(), ContractError> {
        for dependency in canonical_job
            .needs
            .iter()
            .filter(|dependency| dependency.starts_with(BUN_JOB_PREFIX))
        {
            let Some(selected) = identities.get(dependency) else {
                continue;
            };
            let candidates = self
                .id_map
                .get(dependency)
                .ok_or_else(|| plan_error("dependency_output_missing", dependency))?;
            if candidates.len() == 1 && candidates[0] == *selected {
                continue;
            }
            let Some(position) = projected_job
                .needs
                .iter()
                .position(|need| candidates.contains(need))
            else {
                return Err(plan_error("projected_dependency_missing", dependency));
            };
            projected_job
                .needs
                .retain(|need| !candidates.contains(need));
            let insert_at = position.min(projected_job.needs.len());
            projected_job.needs.insert(insert_at, selected.clone());
        }
        Ok(())
    }
}

fn lane_outputs(logical_id: &str, placement: Placement) -> Vec<LaneOutput> {
    match placement {
        Placement::HostedOnly => vec![LaneOutput {
            id: logical_id.to_owned(),
            lane: PhysicalLane::Hosted,
            paired: false,
        }],
        Placement::ScaleSetOnly => vec![LaneOutput {
            id: logical_id.to_owned(),
            lane: PhysicalLane::Scale,
            paired: false,
        }],
        Placement::Both => vec![
            LaneOutput {
                id: format!("{logical_id}{HOSTED_SUFFIX}"),
                lane: PhysicalLane::Hosted,
                paired: true,
            },
            LaneOutput {
                id: format!("{logical_id}{SCALE_SUFFIX}"),
                lane: PhysicalLane::Scale,
                paired: true,
            },
        ],
    }
}

fn project_condition(
    canonical_job: &Job,
    identities: &BTreeMap<String, String>,
    projected_job: &mut Job,
) {
    let Some(condition) = projected_job.condition.as_mut() else {
        return;
    };
    for dependency in &canonical_job.needs {
        if let Some(output_id) = identities.get(dependency) {
            let source = format!("needs.{dependency}.result");
            if condition.contains(source.as_str()) {
                let target = format!("needs.{output_id}.result");
                *condition = condition.replace(source.as_str(), target.as_str());
            }
        }
    }
}

fn project_steps(identities: &BTreeMap<String, String>, output: &LaneOutput, job: &mut Job) {
    for step in &mut job.steps {
        match &mut step.kind {
            StepKind::Action { uses, with, .. } if uses.starts_with("actions/upload-artifact@") => {
                if let Some(name) = with.get_mut("name") {
                    project_upload_name(name, identities, output);
                }
                if let Some(path) = with.get_mut("path") {
                    project_asset_path(path, identities);
                }
            }
            StepKind::Action { uses, with, .. }
                if uses.starts_with("actions/download-artifact@") =>
            {
                if let Some(pattern) = with.get_mut("pattern") {
                    project_blob_pattern(pattern, identities);
                }
                if let Some(name) = with.get_mut("name") {
                    project_asset_name(name, identities);
                }
                if let Some(path) = with.get_mut("path") {
                    project_asset_path(path, identities);
                }
            }
            StepKind::Internal { operation, env }
                if matches!(
                    operation.as_str(),
                    super::super::BUN_ASSETS_EXPORT_OPERATION
                        | super::super::BUN_ASSETS_PREPARE_OPERATION
                        | super::super::BUN_ASSETS_IMPORT_OPERATION
                ) =>
            {
                if let Some(asset_id) = env.get_mut("VELNOR_BUN_ASSET_ID")
                    && let Some(output_id) = identities.get(asset_id.as_str())
                {
                    output_id.clone_into(asset_id);
                }
            }
            _ => {}
        }
    }
}

fn project_upload_name(
    name: &mut String,
    identities: &BTreeMap<String, String>,
    output: &LaneOutput,
) {
    if let Some(value) = project_leg_identity(name, "velnor-pw-blob-", identities) {
        value.clone_into(name);
    } else if let Some(value) = project_leg_identity(name, "velnor-pw-results-", identities) {
        value.clone_into(name);
    } else if let Some(value) = project_asset_identity(name, identities) {
        value.clone_into(name);
    } else if name
        .strip_prefix("velnor-pw-report-")
        .is_some_and(|suite_id| identities.contains_key(suite_id))
        && output.paired
    {
        name.push_str(match output.lane {
            PhysicalLane::Hosted => HOSTED_SUFFIX,
            PhysicalLane::Scale => SCALE_SUFFIX,
        });
    }
}

fn project_blob_pattern(pattern: &mut String, identities: &BTreeMap<String, String>) {
    if let Some(value) = project_leg_identity(pattern, "velnor-pw-blob-", identities) {
        value.clone_into(pattern);
    }
}

fn project_leg_identity(
    value: &str,
    prefix: &str,
    identities: &BTreeMap<String, String>,
) -> Option<String> {
    let rest = value.strip_prefix(prefix)?;
    for (logical_id, output_id) in identities {
        if let Some(tail) = rest.strip_prefix(logical_id.as_str())
            && tail.starts_with("-leg-")
        {
            return Some(format!("{prefix}{output_id}{tail}"));
        }
    }
    None
}

fn project_asset_name(name: &mut String, identities: &BTreeMap<String, String>) {
    if let Some(value) = project_asset_identity(name, identities) {
        value.clone_into(name);
    }
}

fn project_asset_identity(value: &str, identities: &BTreeMap<String, String>) -> Option<String> {
    let rest = value.strip_prefix("velnor-pw-assets-")?;
    let logical_id = rest.strip_suffix(".tar.gz")?;
    let output_id = identities.get(logical_id)?;
    Some(format!("velnor-pw-assets-{output_id}.tar.gz"))
}

fn project_asset_path(path: &mut String, identities: &BTreeMap<String, String>) {
    if !path.contains("/velnor/bun-assets/") {
        return;
    }
    for (logical_id, output_id) in identities {
        let archive = format!("velnor-pw-assets-{logical_id}.tar.gz");
        let projected_archive = format!("velnor-pw-assets-{output_id}.tar.gz");
        let logical_directory = format!("/{logical_id}/");
        let output_directory = format!("/{output_id}/");
        let logical_stage = format!("/velnor/bun-assets/{logical_id}");
        let output_stage = format!("/velnor/bun-assets/{output_id}");
        if let Some(projected_stage) = path
            .strip_suffix(logical_stage.as_str())
            .map(|prefix| format!("{prefix}{output_stage}"))
        {
            *path = projected_stage;
        }
        *path = path
            .replace(archive.as_str(), projected_archive.as_str())
            .replace(logical_directory.as_str(), output_directory.as_str());
    }
}

fn plan_error(problem: &str, logical_id: &str) -> ContractError {
    ContractError::identity("bun_lane_plan", format!("{problem}:{logical_id}"))
}
