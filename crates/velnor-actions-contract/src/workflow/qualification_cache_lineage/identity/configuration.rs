use serde::Serialize;

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::Plan;

use super::{canonical_adapter_metadata, logical_package_id};

#[derive(Serialize)]
struct StaticMatrixEntry<'a> {
    id: &'a str,
    matrix_key: &'a str,
    stack_id: &'a str,
    task_id: &'a str,
    run: &'a str,
    task_digest: &'a str,
    adapter_metadata: serde_json::Value,
    execute_task_ids: &'a crate::workflow::ExecuteTaskIds,
    job_id: &'a str,
    cache_ids: &'a Option<crate::workflow::EntryCacheIds>,
    toolchain_inputs: &'a Option<crate::cachekey::ToolchainInputs>,
    toolchain_id: &'a Option<String>,
    declared_outputs: &'a [String],
}

#[derive(Serialize)]
struct StaticObligation<'a> {
    task_id: &'a str,
    decision: crate::workflow::ObligationDecision,
    task_digest: &'a str,
}

#[derive(Serialize)]
struct StaticPackage<'a> {
    logical_id: String,
    name: &'a str,
    manifest: &'a str,
    selected: bool,
    reasons: &'a [String],
    tasks: &'a [String],
}

#[derive(Serialize)]
struct PlanConfiguration<'a> {
    runner: &'a crate::workflow::PlanRunner,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale_set_image_source_sha: Option<&'a str>,
    generator: &'a crate::workflow::PlanGenerator,
    packages: Vec<StaticPackage<'a>>,
    obligations: Vec<StaticObligation<'a>>,
    task_ids: &'a [String],
    edges: &'a [crate::graph::TaskEdge],
    matrix: Vec<StaticMatrixEntry<'a>>,
}

/// Derive the stable configuration commitment shared by all runs in a campaign.
pub(crate) fn configuration_commitment(plan: &Plan) -> Result<String, ContractError> {
    let mut packages: Vec<_> = plan
        .packages
        .iter()
        .map(|package| StaticPackage {
            logical_id: logical_package_id(package),
            name: &package.name,
            manifest: &package.manifest,
            selected: package.selected,
            reasons: &package.reasons,
            tasks: &package.tasks,
        })
        .collect();
    packages.sort_by(|left, right| left.logical_id.cmp(&right.logical_id));
    if packages
        .windows(2)
        .any(|pair| pair[0].logical_id == pair[1].logical_id)
    {
        return Err(ContractError::identity(
            "qualification.configuration.packages",
            "logical_package_collision",
        ));
    }
    let obligations = plan
        .obligations
        .iter()
        .map(|obligation| StaticObligation {
            task_id: &obligation.task_id,
            decision: obligation.decision,
            task_digest: &obligation.task_digest,
        })
        .collect();
    let matrix = plan
        .matrix
        .include
        .iter()
        .map(|entry| {
            Ok(StaticMatrixEntry {
                id: &entry.id,
                matrix_key: &entry.matrix_key,
                stack_id: &entry.stack_id,
                task_id: &entry.task_id,
                run: &entry.run,
                task_digest: &entry.task_digest,
                adapter_metadata: canonical_adapter_metadata(plan, &entry.adapter_metadata)?,
                execute_task_ids: &entry.execute_task_ids,
                job_id: &entry.job_id,
                cache_ids: &entry.cache_ids,
                toolchain_inputs: &entry.toolchain_inputs,
                toolchain_id: &entry.toolchain_id,
                declared_outputs: &entry.declared_outputs,
            })
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    let configuration = PlanConfiguration {
        runner: &plan.runner,
        scale_set_image_source_sha: plan
            .qualification
            .as_ref()
            .and_then(|context| context.scale_set_image_source_sha.as_deref()),
        generator: &plan.generator,
        packages,
        obligations,
        task_ids: &plan.task_ids,
        edges: &plan.edges,
        matrix,
    };
    Ok(digest_b3(&canonical_json_bytes(&configuration)?))
}
