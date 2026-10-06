//! Helper domain capture happens after its genuine current-source build attaches.

use std::collections::BTreeMap;
use velnor_actions_contract::{MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity, Step};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::{MiseSetup, preseed_build_step, preseed_verify_step};

use super::{MbxFinalization, MbxUnsupportedDomain, QualifiedMbxAction, invalid};
use crate::{OrchestratorError, workflow::WorkflowPlan};

/// Reconstruct the attached helper computation; returns detached drafts only.
pub(crate) fn finalize_helper(
    workflow: &WorkflowPlan,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
) -> Result<MbxFinalization, OrchestratorError> {
    if version != env!("CARGO_PKG_VERSION") {
        return Err(invalid("helper_generator_version"));
    }
    let mut result = MbxFinalization::default();
    let Some(plan) = workflow.ir.jobs.get("plan") else {
        return Err(invalid("helper_plan_missing"));
    };
    if !workflow.context.preseed {
        return Ok(pending("mbx_helper_current_source_build_pending"));
    }
    let (build, probe) = attached_recipe(plan, catalog)?;
    let action = match QualifiedMbxAction::require_comparison_export() {
        Ok(action) => action,
        Err(error) => return Ok(pending(&error.to_string())),
    };
    let host = crate::workloads::host_for_runner(&plan.runs_on)?;
    let owner = match super::QualifiedDistribution::require_for_generator(
        super::DistributionTool::Mbx,
        host,
        super::DistributionRequirement::MbxTransport,
    ) {
        Ok(owner) => owner,
        Err(error) => return Ok(pending(&error.to_string())),
    };
    if catalog.version(PinnedTool::MrBoxington) != owner.version() {
        return Ok(pending("mbx_helper_probe_owner_identity_pending"));
    }
    if !velnor_actions_workflow_renderer::early_plan::has_early_plan(plan) {
        return Ok(pending("mbx_helper_source_selection_pending"));
    }
    let (installation, tool_descriptor) =
        super::installation("plan", plan, setup, &workflow.context.source_helpers)?;
    let context = crate::mbx_partition::ToolContext::for_installation(
        catalog,
        &tool_descriptor,
        installation,
        setup,
        version,
    )?
    .canonical()?;
    let descriptor = helper_descriptor(
        plan,
        &context,
        &tool_descriptor.target,
        &build,
        &probe,
        version,
        action,
        &owner,
    )?;
    super::finish_draft(
        &mut result,
        descriptor,
        context,
        catalog,
        setup,
        version,
        action,
        &owner,
    )?;
    result
        .unsupported
        .extend(pending("mbx_original_receipt_quarantine_admission_unqualified").unsupported);
    Ok(result)
}

fn attached_recipe(
    plan: &velnor_actions_contract::Job,
    catalog: &ToolCatalog,
) -> Result<(Step, Step), OrchestratorError> {
    let homes = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let build_argv = crate::vectors::candidate_build_argv(catalog)?;
    let probe_argv = crate::vectors::mbx_probe_argv(catalog)?;
    let build = preseed_build_step(&build_argv, &homes)?;
    let probe = preseed_verify_step(
        &probe_argv,
        catalog.version(PinnedTool::MrBoxington),
        &homes,
    )?;
    let build_at = exact_step(&plan.steps, &build)?;
    let probe_at = exact_step(&plan.steps, &probe)?;
    if build_at >= probe_at {
        return Err(invalid("helper_probe_precedes_build"));
    }
    Ok((build, probe))
}

#[expect(
    clippy::too_many_arguments,
    reason = "actual helper source computation has complete owner inputs"
)]
fn helper_descriptor(
    plan: &velnor_actions_contract::Job,
    context: &str,
    target: &str,
    build: &Step,
    probe: &Step,
    version: &str,
    action: QualifiedMbxAction,
    owner: &super::QualifiedDistribution,
) -> Result<MbxExportDescriptor, OrchestratorError> {
    let bytes = velnor_actions_contract::canonical::canonical_json_bytes(&(
        "velnor-mbx-attached-current-source-helper-domain-v1",
        context,
        build,
        probe,
        version,
        "checkout-root-release-velnor-actions-cli-velnor-actions",
    ))?;
    let descriptor = MbxExportDescriptor {
        domain: MbxCacheDomain::Helper,
        producer_job_id: "plan".to_owned(),
        runs_on: plan.runs_on.clone(),
        target: target.to_owned(),
        workspace_roots: vec![".".to_owned()],
        configuration_digest: velnor_actions_contract::canonical::digest_b3(&bytes),
        task_digests: BTreeMap::new(),
        owner: MbxOwnerIdentity {
            version: owner.version().to_owned(),
            binary_sha256: owner.binary_sha256().to_owned(),
            qualification_identity: owner.qualification_digest(),
            source_sha: owner.source_commit().to_owned(),
        },
        action_sha: action.source_commit().to_owned(),
    };
    descriptor.validate()?;
    Ok(descriptor)
}

fn exact_step(steps: &[Step], expected: &Step) -> Result<usize, OrchestratorError> {
    let mut matches = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| *step == expected);
    let (position, _) = matches
        .next()
        .ok_or_else(|| invalid("helper_current_source_step_changed"))?;
    if matches.next().is_some() {
        return Err(invalid("helper_current_source_step_duplicated"));
    }
    Ok(position)
}

fn pending(reason: &str) -> MbxFinalization {
    MbxFinalization {
        unsupported: vec![MbxUnsupportedDomain {
            domain: MbxCacheDomain::Helper,
            job_id: "plan".to_owned(),
            reason: reason.to_owned(),
        }],
        ..MbxFinalization::default()
    }
}

#[cfg(test)]
#[path = "workflow_mbx_helper_finalize_tests.rs"]
mod tests;
