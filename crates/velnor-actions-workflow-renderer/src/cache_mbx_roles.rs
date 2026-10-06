//! Closed structural MBX writer drafts; source owners independently grant authority.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ActionOutput, CacheMode, CompiledSourceHelper, Job, PermissionLevel, Permissions,
    PureMbxProducer, SourceBoundOperation, Step, StepKind,
    workflow::{JobOutput, StepOutputRef},
};

/// Construct a reviewable draft without granting source or publication authority.
/// Records are preparation, artifact admission, data verification, terminal report.
/// # Errors
/// Rejects changed operation boundaries and executable payload transport.
pub fn draft_producer_steps(
    metadata: &PureMbxProducer,
    records: &[CompiledSourceHelper; 4],
) -> Result<Vec<Step>, RenderError> {
    metadata.validate().map_err(RenderError::Contract)?;
    let specifications = [
        (
            SourceBoundOperation::MbxProducerPrepare,
            &metadata.installation_step,
            "Prepare fresh verified MBX owner",
        ),
        (
            SourceBoundOperation::MbxArtifactAdmission,
            &metadata.admission_step,
            "Admit immutable native MBX export",
        ),
        (
            SourceBoundOperation::MbxBundleVerify,
            &metadata.verification_step,
            "Verify native MBX bundle as data",
        ),
        (
            SourceBoundOperation::MbxProducerReport,
            &metadata.report_step,
            "Report native MBX cache availability",
        ),
    ];
    let mut helpers = Vec::new();
    for (record, (operation, id, name)) in records.iter().zip(specifications) {
        if record.invocation().descriptor().operation() != operation {
            return Err(invalid("foreign_helper_operation"));
        }
        let mut step =
            crate::source_helper::source_helper_step(name, record, record.environment().clone())?;
        step.id = Some(id.clone());
        helpers.push(step);
    }
    let [prepare, admission, verify, mut report]: [Step; 4] = helpers
        .try_into()
        .map_err(|_| invalid("incomplete_helper_sequence"))?;
    report.condition = Some("always()".to_owned());
    Ok(vec![
        prepare,
        admission,
        verify,
        transport_step(metadata, false)?,
        transport_step(metadata, true)?,
        report,
    ])
}

fn transport_step(meta: &PureMbxProducer, lookup: bool) -> Result<Step, RenderError> {
    let mut with = BTreeMap::from([
        (
            "path".to_owned(),
            meta.descriptor
                .bundle_root()
                .map_err(RenderError::Contract)?,
        ),
        (
            "key".to_owned(),
            meta.save_key().map_err(RenderError::Contract)?,
        ),
    ]);
    if lookup {
        with.insert("lookup-only".to_owned(), "true".to_owned());
    }
    let mut step = crate::steps::action_step(
        if lookup {
            "Verify exact native MBX cache publication"
        } else {
            "Save verified useful native MBX bundle"
        },
        if lookup {
            crate::cache_steps::TOOLS_RESTORE_USES
        } else {
            crate::cache_steps::TOOLS_SAVE_USES
        },
        with,
    )?;
    step.id = Some(if lookup {
        meta.publication_step.clone()
    } else {
        meta.save_step.clone()
    });
    step.condition = Some(if lookup {
        meta.publication_condition()
    } else {
        meta.save_condition()
    });
    Ok(step)
}

/// Terminal advisory evidence remains independent from task completion.
#[must_use]
pub fn producer_outputs(meta: &PureMbxProducer) -> Vec<JobOutput> {
    [
        ActionOutput::CacheAvailable,
        ActionOutput::Verified,
        ActionOutput::SourceIdentity,
        ActionOutput::Error,
    ]
    .into_iter()
    .map(|output| JobOutput {
        name: output.as_str().to_owned(),
        value: StepOutputRef {
            step_id: meta.report_step.clone(),
            output,
        },
    })
    .collect()
}

/// Validate the exact structural draft and its complete registered helper closure.
/// This validation grants no source, signing or warm publication authority.
pub(crate) fn validate_mbx_producer(
    job: &Job,
    records: &[CompiledSourceHelper],
) -> Result<bool, RenderError> {
    let Some(meta) = &job.mbx_producer else {
        if job.steps.iter().any(is_writer_helper) {
            return Err(invalid("missing_role"));
        }
        return Ok(false);
    };
    meta.validate().map_err(RenderError::Contract)?;
    validate_job_boundary(job, meta)?;
    let helpers = [
        registered_helper(job, &meta.installation_step, records)?,
        registered_helper(job, &meta.admission_step, records)?,
        registered_helper(job, &meta.verification_step, records)?,
        registered_helper(job, &meta.report_step, records)?,
    ];
    if job.steps != draft_producer_steps(meta, &helpers)? {
        return Err(invalid("mixed_or_changed_computation"));
    }
    velnor_actions_contract::workflow::step::validate_step_ids(&job.steps)
        .map_err(RenderError::Contract)?;
    Ok(true)
}

fn registered_helper(
    job: &Job,
    id: &velnor_actions_contract::StepId,
    records: &[CompiledSourceHelper],
) -> Result<CompiledSourceHelper, RenderError> {
    let steps = job
        .steps
        .iter()
        .filter(|step| step.id.as_ref() == Some(id))
        .collect::<Vec<_>>();
    let [step] = steps.as_slice() else {
        return Err(invalid("missing_or_ambiguous_helper_binding"));
    };
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Err(invalid("uncompiled_helper_binding"));
    };
    let matches = records
        .iter()
        .filter(|record| record.invocation() == invocation && record.environment() == env)
        .collect::<Vec<_>>();
    let [record] = matches.as_slice() else {
        return Err(invalid("missing_or_ambiguous_owner_source"));
    };
    Ok((*record).clone())
}

fn validate_job_boundary(job: &Job, meta: &PureMbxProducer) -> Result<(), RenderError> {
    if job.condition.as_ref() != Some(&meta.condition())
        || job.needs != meta.needs()
        || job.outputs != producer_outputs(meta)
        || job.cache_mode != Some(CacheMode::Write)
        || job.runs_on != meta.descriptor.runs_on
        || velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
            != Some(meta.descriptor.target.as_str())
    {
        return Err(invalid("scheduling_or_cohort_changed"));
    }
    if job.source_producer.is_some()
        || job.tool_producer.is_some()
        || job.native_pages_deploy.is_some()
        || job.native_publish.is_some()
        || job.environment.is_some()
        || job.permissions.as_ref() != Some(&writer_permissions())
    {
        return Err(invalid("foreign_authority"));
    }
    Ok(())
}

fn writer_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        actions: PermissionLevel::Read,
        ..Permissions::default()
    }
}

fn is_writer_helper(step: &Step) -> bool {
    matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
        if matches!(invocation.descriptor().operation(),
            SourceBoundOperation::MbxProducerPrepare
                | SourceBoundOperation::MbxArtifactAdmission
                | SourceBoundOperation::MbxBundleVerify
                | SourceBoundOperation::MbxProducerReport))
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("mbx_producer_{reason}"))
}
