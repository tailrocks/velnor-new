// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{PreparedWorkflow, WorkflowBuildInput};
use crate::OrchestratorError;
use std::collections::BTreeMap;
use velnor_actions_contract::{Job, canonical_json_bytes};
use velnor_actions_workflow_renderer::native_authority::{
    NativeAuthority, NativeAuthorityError, NativeAuthorityInput, NativeRustProvider,
    NativeRustSource,
};

#[path = "workflow_native_facts.rs"]
mod facts;

#[path = "workflow_native_graph.rs"]
mod graph;

pub(super) fn build(
    input: &WorkflowBuildInput<'_>,
    prepared: &PreparedWorkflow,
    jobs: &mut BTreeMap<String, Job>,
) -> Result<NativeAuthority, OrchestratorError> {
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(input.root, input.config)?;
    let pins = facts::tool_pins(input.config, &catalog);
    let workspaces = facts::cargo_workspaces(input);
    let format = facts::workspace_format(input)?;
    let gradle_roots = facts::gradle_roots(input);
    let rust_tasks = facts::rust_tasks(input)?;
    let tofu_tasks = facts::tofu_tasks(input)?;
    let generated_sources = facts::generated_sources(input, prepared)?;
    let workflow_sources = facts::workflow_sources(input, prepared);
    let facts = graph::graph(input, prepared, jobs)?;
    let native = NativeAuthority::build(NativeAuthorityInput {
        config: input.config,
        runner: input.label,
        published_generator_lock: input.published_generator_lock,
        consumer_manifest_json: input.discovery.consumer_manifest_json.as_deref(),
        tool_pins: &pins,
        rust_source: if catalog.exports_rustup_toolchain() {
            NativeRustSource::ConfigOrDefault
        } else {
            NativeRustSource::ToolchainFile
        },
        rust_provider: if catalog.rust_mr_boxington() {
            NativeRustProvider::MrBoxington
        } else {
            NativeRustProvider::Rustup
        },
        cargo_workspaces: &workspaces,
        workflow_sources: &workflow_sources,
        generated_sources: &generated_sources,
        rust_tasks: &rust_tasks,
        tofu_tasks: &tofu_tasks,
        gradle_roots: &gradle_roots,
        logical_jobs: &facts,
        workspace_format: format.as_ref(),
        tools: prepared.needs,
        rust_detected: super::plan_uses_rust(input.discovery, prepared.policy),
    })
    .map_err(map_error)?;
    replace_native(
        jobs,
        velnor_actions_workflow_renderer::render::FINAL_JOB_ID,
        native.required_job(),
    )?;
    if let Some(plan) = native.producer_plan_job() {
        replace_native(
            jobs,
            velnor_actions_workflow_renderer::render::PLAN_JOB_ID,
            plan,
        )?;
    }
    Ok(native)
}

fn replace_native(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    native: Job,
) -> Result<(), OrchestratorError> {
    let existing = jobs
        .get(id)
        .ok_or_else(|| problem("native_authority_job_missing"))?;
    if canonical_json_bytes(existing)? != canonical_json_bytes(&native)? {
        return Err(problem(format!("native_authority_discovery_mismatch:{id}")));
    }
    jobs.insert(id.to_owned(), native);
    Ok(())
}

fn map_error(error: NativeAuthorityError) -> OrchestratorError {
    match error {
        NativeAuthorityError::Contract(error) => error.into(),
        NativeAuthorityError::Render(error) => error.into(),
        NativeAuthorityError::Release(error) => {
            crate::local_release_manifest::map_release_helper_error(error)
        }
        NativeAuthorityError::Job(error) => {
            crate::workflow_jobs::map_native_job_builder_error(error)
        }
        NativeAuthorityError::Support(error) => crate::verify::map_error(error),
        NativeAuthorityError::Tool(error) => problem(error.to_string()),
    }
}

fn problem(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}
