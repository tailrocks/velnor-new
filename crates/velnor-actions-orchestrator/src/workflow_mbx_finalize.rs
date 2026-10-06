//! MBX source drafts follow canonical tool normalization, never runtime admission.

use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, Job, MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity, ProposedTask,
    Step, StepKind, ToolCacheDomain, ToolProducerSelection,
};
use velnor_actions_mise::{
    ToolCatalog,
    catalog::mbx_action_authority::QualifiedMbxAction,
    catalog::qualification::{DistributionRequirement, DistributionTool, QualifiedDistribution},
};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::{OrchestratorError, discover::Discovery};

#[path = "workflow_mbx_restore_draft.rs"]
mod restore;

#[path = "workflow_mbx_helper_finalize.rs"]
mod helper;
pub(crate) use helper::finalize_helper;

#[path = "workflow_mbx_review.rs"]
mod review;

/// Retained explanation for a selected domain whose cache cannot be activated.
#[derive(Debug, Clone)]
pub(crate) struct MbxUnsupportedDomain {
    pub domain: MbxCacheDomain,
    pub job_id: String,
    pub reason: String,
}

/// Reviewable source computation. These steps are never inserted into a workload.
#[derive(Debug, Clone)]
pub(crate) struct MbxDomainDraft {
    pub job_id: String,
    pub descriptor: MbxExportDescriptor,
    pub tool_context: String,
    pub restore: Step,
    pub export: Step,
    pub upload: Step,
    pub producer: Job,
}

/// Detached source candidates and explicit cold outcomes; no restoration grant.
#[derive(Debug, Clone, Default)]
pub(crate) struct MbxFinalization {
    pub source_helpers: Vec<CompiledSourceHelper>,
    pub receipt_drafts: Vec<crate::cache_producer_workflow::DraftCacheProducerWorkflow>,
    pub domains: Vec<MbxDomainDraft>,
    pub unsupported: Vec<MbxUnsupportedDomain>,
}

/// Finalize every selected MBX validation domain from its actual normalized job.
#[expect(
    clippy::too_many_arguments,
    reason = "named graph owner supplies exact source inputs"
)]
pub(crate) fn finalize(
    jobs: &BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
    authority: &[CompiledSourceHelper],
    fetch_roots: &[String],
) -> Result<MbxFinalization, OrchestratorError> {
    if version != env!("CARGO_PKG_VERSION") {
        return Err(invalid("generator_version"));
    }
    let grouped = crate::crate_job_ids::group_runnable(&discovery.proposals);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    let mut result = MbxFinalization::default();
    for (key, tasks) in &grouped {
        if !tasks.iter().any(|task| crate::crate_jobs::is_mbx(task)) {
            continue;
        }
        let id = assigned
            .get(key)
            .ok_or_else(|| invalid("missing_job_identity"))?;
        let job = jobs
            .get(id)
            .ok_or_else(|| invalid("missing_normalized_job"))?;
        let action = match QualifiedMbxAction::require_comparison_export() {
            Ok(action) => action,
            Err(error) => {
                cold(&mut result, id, error.to_string());
                continue;
            }
        };
        let host = crate::workloads::host_for_runner(&job.runs_on)?;
        let owner = match QualifiedDistribution::require_for_generator(
            DistributionTool::Mbx,
            host,
            DistributionRequirement::MbxTransport,
        ) {
            Ok(owner) => owner,
            Err(error) => {
                cold(&mut result, id, error.to_string());
                continue;
            }
        };
        let scoped = crate::workloads::catalog_for_configuration(catalog, &key.1)?;
        let roots = crate::source_prep::selected_fetch_roots(discovery, tasks, fetch_roots);
        build_draft(
            &mut result,
            id,
            job,
            tasks,
            &key.1,
            &roots,
            &scoped,
            setup,
            version,
            authority,
            action,
            &owner,
        )?;
        cold(
            &mut result,
            id,
            "mbx_original_receipt_quarantine_admission_unqualified".to_owned(),
        );
    }
    Ok(result)
}

#[expect(
    clippy::too_many_arguments,
    reason = "closed source owner reconstructs complete domain"
)]
fn build_draft(
    result: &mut MbxFinalization,
    id: &str,
    job: &Job,
    tasks: &[&ProposedTask],
    configuration: &str,
    roots: &[String],
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
    authority: &[CompiledSourceHelper],
    action: QualifiedMbxAction,
    owner: &QualifiedDistribution,
) -> Result<(), OrchestratorError> {
    let (installation, tool_descriptor) = installation(id, job, setup, authority)?;
    let context = crate::mbx_partition::ToolContext::for_installation(
        catalog,
        &tool_descriptor,
        installation,
        setup,
        version,
    )?
    .canonical()?;
    let descriptor = descriptor(
        id,
        job,
        tasks,
        configuration,
        roots,
        catalog,
        &context,
        &tool_descriptor.target,
        action,
        owner,
    )?;
    finish_draft(
        result, descriptor, context, catalog, setup, version, action, owner,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "same closed source factory serves both MBX domains"
)]
fn finish_draft(
    result: &mut MbxFinalization,
    descriptor: MbxExportDescriptor,
    context: String,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
    action: QualifiedMbxAction,
    owner: &QualifiedDistribution,
) -> Result<(), OrchestratorError> {
    let export = crate::mbx_export::from_descriptor(&descriptor, catalog, version)?;
    let producer = crate::mbx_producer::draft_mbx_producer(
        &descriptor,
        &ToolProducerSelection {
            tasks: descriptor.task_digests.keys().cloned().collect(),
            cargo_fallback: descriptor.domain == MbxCacheDomain::Helper,
            unconditional: false,
        },
        catalog,
        setup,
        version,
    )?;
    result.domains.push(MbxDomainDraft {
        job_id: descriptor.producer_job_id.clone(),
        restore: restore::restore_draft(&descriptor, catalog, action, owner)?,
        export: crate::mbx_export::export_step(&export)?,
        upload: crate::mbx_export::upload_step(&descriptor)?,
        descriptor,
        tool_context: context,
        producer: producer.original().clone(),
    });
    result.source_helpers.push(export);
    result
        .source_helpers
        .extend_from_slice(producer.source_helpers());
    result.receipt_drafts.push(producer.receipt_candidate()?);
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "complete source domain has explicit owner inputs"
)]
fn descriptor(
    id: &str,
    job: &Job,
    tasks: &[&ProposedTask],
    configuration: &str,
    roots: &[String],
    catalog: &ToolCatalog,
    context: &str,
    target: &str,
    action: QualifiedMbxAction,
    owner: &QualifiedDistribution,
) -> Result<MbxExportDescriptor, OrchestratorError> {
    let (obligations, _) = crate::crate_jobs::compile_obligations(tasks, catalog, &job.runs_on)?;
    let task_digests: BTreeMap<_, _> = obligations
        .iter()
        .map(|obligation| (obligation.task_id.clone(), obligation.task_digest.clone()))
        .collect();
    let bytes = velnor_actions_contract::canonical::canonical_json_bytes(&(
        "velnor-mbx-normalized-source-domain-v1",
        &context,
        configuration,
        roots,
        &task_digests,
    ))?;
    let mut workspace_roots: Vec<_> = roots
        .iter()
        .map(|root| {
            if root.is_empty() {
                ".".to_owned()
            } else {
                root.clone()
            }
        })
        .collect();
    workspace_roots.sort();
    workspace_roots.dedup();
    let descriptor = MbxExportDescriptor {
        domain: MbxCacheDomain::Validation,
        producer_job_id: id.to_owned(),
        runs_on: job.runs_on.clone(),
        target: target.to_owned(),
        workspace_roots,
        configuration_digest: velnor_actions_contract::canonical::digest_b3(&bytes),
        task_digests,
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

fn installation<'a>(
    id: &str,
    job: &Job,
    setup: &MiseSetup,
    authority: &'a [CompiledSourceHelper],
) -> Result<
    (
        &'a CompiledSourceHelper,
        velnor_actions_contract::ToolCacheDescriptor,
    ),
    OrchestratorError,
> {
    let target = velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
        .ok_or_else(|| invalid("unknown_target"))?;
    let descriptors =
        velnor_actions_workflow_renderer::tool_producer_steps::consumer_tool_descriptors(
            id, job, setup, target, authority,
        )?;
    let mut full = descriptors
        .into_iter()
        .filter(|descriptor| descriptor.domain == ToolCacheDomain::Full);
    let descriptor = full
        .next()
        .ok_or_else(|| invalid("missing_full_tool_descriptor"))?;
    if full.next().is_some() {
        return Err(invalid("multiple_full_tool_descriptors"));
    }
    let mut candidates = authority.iter().filter(|record| {
        job.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
            if invocation == record.invocation() && env == record.environment()
            && invocation.installed_selectors() == descriptor.selectors)
        })
    });
    let record = candidates
        .next()
        .ok_or_else(|| invalid("missing_exact_full_installation"))?;
    if candidates.next().is_some() {
        return Err(invalid("ambiguous_full_installation"));
    }
    Ok((record, descriptor))
}

fn cold(result: &mut MbxFinalization, id: &str, reason: String) {
    result.unsupported.push(MbxUnsupportedDomain {
        domain: MbxCacheDomain::Validation,
        job_id: id.to_owned(),
        reason,
    });
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("mbx_finalize_{reason}"),
    }
}
