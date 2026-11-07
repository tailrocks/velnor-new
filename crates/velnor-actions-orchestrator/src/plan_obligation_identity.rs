// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::{ProposedTask, Stack, StackExtension};
use velnor_actions_rust::extension_for_proposal;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{ExtensionBundle, extension_bundle_with_snapshot};
use crate::internal_plan::snapshot::canonical_digest;
use crate::internal_plan::{IdentityInputs, nextest_config_for, task_identity_digest};

use super::{
    GroupInputs, adapter_metadata, evidence_for_group, execute_ids, record_lane_target_dir,
    record_task_cache, task_digest, toolchain_id, wire_w2,
};
use velnor_actions_contract::{MatrixEntry, PlanObligation, PlannedPlatform};

pub(super) struct PlannedIdentity {
    pub(super) bundle: ExtensionBundle,
    pub(super) reuse_eligible: bool,
    pub(super) closure_digest: String,
    pub(super) input_digest: String,
}

pub(super) fn extension_for_task(
    task: &ProposedTask,
    root: &Path,
    bundle: &ExtensionBundle,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(StackExtension, bool), OrchestratorError> {
    let stack = Stack::require_known(&task.stack_id).map_err(internal_contract)?;
    if stack == Stack::Mise {
        return Err(internal("named_check_definition_required"));
    }
    if stack == Stack::Tofu {
        let ext = crate::internal_plan::tofu_extension_for(task, root, bundle, reads)
            .map_err(internal_contract)?;
        return Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()));
    }
    let ext = extension_for_proposal(task, &bundle.inputs()).map_err(internal_contract)?;
    Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()))
}

pub(super) fn planned_identity(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<PlannedIdentity, OrchestratorError> {
    let task = inputs.task;
    let manifest = task.identity.unit_path.clone();
    let nextest_config = nextest_config_for(inputs.discovery, task);
    let bundle = extension_bundle_with_snapshot(
        inputs.snapshot,
        inputs.discovery,
        task,
        Some(inputs.root),
        nextest_config.as_deref(),
    );
    let (extension, reuse_eligible) = extension_for_task(task, inputs.root, &bundle, reads)?;
    let closure = if Stack::from_id(&task.stack_id) == Some(Stack::Rust) {
        inputs
            .snapshot
            .resolve_rust_closure(
                inputs.root,
                task,
                nextest_config.as_deref(),
                bundle.graph_digest(),
                toolchain,
                platform_id,
            )
            .map_err(internal_contract)?
    } else {
        resolve_closure_at_root(
            inputs.root,
            task,
            nextest_config.as_deref(),
            bundle.graph_digest(),
            toolchain,
            platform_id,
            &mut *reads,
        )
        .map_err(internal_contract)?
    };
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv,
        toolchain_id: toolchain,
        platform_id,
        manifest: &manifest,
        generator: inputs.wire.generator,
        extension,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    Ok(PlannedIdentity {
        bundle,
        reuse_eligible,
        closure_digest,
        input_digest,
    })
}

pub(super) fn plan_routed_rust_group(
    inputs: &GroupInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let task = inputs.task;
    let logical = inputs
        .job_owners
        .for_member(task)?
        .ok_or_else(|| internal("crate_job_id_missing"))?;
    let routes = rust_routes(inputs, &logical)?;
    let toolchain = toolchain_id(task, inputs.catalog).map_err(internal_contract)?;
    let (argv, binding_toolchain) =
        crate::tool_catalog_project::task_binding(task, inputs.catalog)?;
    let task_digest =
        task_digest(&task.task_id, &argv, &binding_toolchain).map_err(internal_contract)?;
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)
        .map_err(|err| internal(&err.to_string()))?;
    let mut entries = Vec::new();
    let mut obligation = None;
    for route in routes {
        let target = route
            .target
            .as_deref()
            .ok_or_else(|| internal("paired_rust_target_missing"))?;
        if task.identity.target != "host" && task.identity.target != target {
            return Err(internal("paired_rust_target_mismatch"));
        }
        let platform = PlannedPlatform::new(&route.runner, target).map_err(internal_contract)?;
        let identity = planned_identity(
            inputs,
            &argv,
            &toolchain,
            &platform.platform_id,
            &mut *reads,
        )?;
        let gate = wire_w2::check_archive_identity_with_source(
            task,
            &toolchain,
            &platform.platform_id,
            identity.bundle.config_digest(),
            Some(&identity.closure_digest),
            inputs.catalog,
        )?;
        if !inputs.changed && gate == wire_w2::ArchiveGate::SourceUnbound {
            return Err(internal(&format!(
                "archive_source_unbound:{}",
                task.task_id
            )));
        }
        let reuse = if inputs.changed {
            wire_w2::ReuseOutcome::execute("affected_by_change")
        } else {
            wire_w2::ReuseOutcome::not_affected()
        };
        if route.variant.is_none()
            || route.variant == Some(velnor_actions_contract::LaneVariant::Hosted)
        {
            obligation = Some(PlanObligation {
                task_id: task.task_id.clone(),
                decision: reuse.decision,
                reason: reuse.reason.clone(),
                task_digest: task_digest.clone(),
                input_digest: identity.input_digest.clone(),
                closure_digest: identity.closure_digest.clone(),
                job_id: logical.clone(),
            });
        }
        let context = RustEntryContext {
            inputs,
            task_digest: &task_digest,
            run: &run,
            binding_toolchain: &binding_toolchain,
        };
        entries.push(rust_entry(&context, route, platform, &identity, &reuse)?);
    }
    Ok((
        obligation.ok_or_else(|| internal("paired_hosted_route_missing"))?,
        entries,
    ))
}

fn rust_routes<'a>(
    inputs: &'a GroupInputs<'_>,
    logical: &str,
) -> Result<Vec<&'a velnor_actions_contract::LaneRoute>, OrchestratorError> {
    let routes: Vec<_> = inputs
        .lane_routes
        .iter()
        .filter(|route| {
            route.logical_job_id == logical && route.task_ids.contains(&inputs.task.task_id)
        })
        .collect();
    if routes.is_empty() || routes.len() > 2 {
        return Err(internal("paired_rust_route_missing"));
    }
    Ok(routes)
}

struct RustEntryContext<'a, 'b> {
    inputs: &'a GroupInputs<'b>,
    task_digest: &'a str,
    run: &'a str,
    binding_toolchain: &'a str,
}

fn rust_entry(
    context: &RustEntryContext<'_, '_>,
    route: &velnor_actions_contract::LaneRoute,
    platform: PlannedPlatform,
    identity: &PlannedIdentity,
    reuse: &wire_w2::ReuseOutcome,
) -> Result<MatrixEntry, OrchestratorError> {
    let inputs = context.inputs;
    let task = inputs.task;
    let mut metadata = adapter_metadata(task, evidence_for_group(inputs.discovery, task))
        .map_err(internal_contract)?;
    record_task_cache(
        &mut metadata,
        reuse.task_cache_enabled,
        reuse.task_cache_key.as_deref(),
    );
    let cache_ids = crate::internal_plan::cache_ids_for_platform(
        task,
        &platform.platform_id,
        context.binding_toolchain,
        inputs.catalog,
    )
    .map_err(internal_contract)?;
    record_lane_target_dir(&mut metadata, cache_ids.lane_id());
    let mut entry = MatrixEntry::derive_for_lane(
        &task.stack_id,
        &task.task_id,
        context.run,
        context.task_digest,
        metadata,
        execute_ids(task),
        &identity.input_digest,
        inputs.run_key,
        &route.job_id,
        route.variant,
        platform,
    )
    .map_err(internal_contract)?;
    entry.cache_ids = Some(cache_ids);
    Ok(entry)
}
