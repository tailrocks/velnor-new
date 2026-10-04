//! Closed source roles admit only isolated native production and transport.
use crate::{MiseSetup, RenderError};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::{JobOutput, StepOutputRef};
use velnor_actions_contract::{
    ActionOutput, CacheMode, Job, PermissionLevel, SourceBoundOperation, SourceProducer,
    SourceProducerRole, Step, StepKind, ToolCacheDescriptor,
};

#[path = "cache_source_roles_snapshots.rs"]
mod snapshots;
#[path = "cache_source_roles_tofu.rs"]
mod tofu;
#[path = "cache_source_roles_transport.rs"]
mod transport;

#[cfg(test)]
#[path = "cache_source_roles_adversarial_tests.rs"]
mod adversarial_tests;
#[cfg(test)]
#[path = "cache_source_roles_tests.rs"]
mod tests;

pub(crate) fn validate_source_producer(
    job: &Job,
    setup: &MiseSetup,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<bool, RenderError> {
    let Some(meta) = &job.source_producer else {
        if job.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation().is_source_producer())
        }) {
            return Err(invalid("source_producer_missing_role"));
        }
        return Ok(false);
    };
    meta.validate().map_err(RenderError::Contract)?;
    validate_schedule(job, meta)?;
    let descriptor = meta
        .tool_cache
        .as_ref()
        .ok_or_else(|| invalid("source_producer_bootstrap_descriptor_missing"))?;
    if descriptor.domain != transport::source_domain(meta.role)
        || descriptor.runs_on != job.runs_on
        || velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
            != Some(descriptor.target.as_str())
    {
        return Err(invalid("source_producer_bootstrap_descriptor_changed"));
    }
    let prefix_len = validate_consumer_prefix(job, meta, descriptor, setup)?;
    let install = job
        .steps
        .get(3)
        .ok_or_else(|| invalid("source_producer_tool_installation_missing"))?;
    let installation = if meta.role == SourceProducerRole::Cargo {
        crate::tool_producer_steps::is_rust_source_preparation(descriptor, install, setup, records)?
    } else {
        crate::tool_producer_steps::is_tool_consumer_installation(
            descriptor, install, setup, records,
        )?
    };
    if !installation {
        return Err(invalid("source_producer_tool_installation_changed"));
    }
    tofu::validate(job, meta, records)?;
    snapshots::validate(job, meta, prefix_len)?;
    if job.environment.is_some()
        || !job.permissions.as_ref().is_some_and(no_permissions)
        || job.tool_producer.is_some()
        || job.native_pages_deploy.is_some()
        || job.native_publish.is_some()
    {
        return Err(invalid("source_producer_private_authority"));
    }
    for step in job.steps.iter().skip(prefix_len) {
        if !transport::allowed_step(step, meta) {
            return Err(invalid("source_producer_mixed_computation"));
        }
    }
    validate_evidence(job, meta)
}

fn validate_schedule(job: &Job, meta: &SourceProducer) -> Result<(), RenderError> {
    let domain = transport::source_domain(meta.role);
    if job.condition.as_deref() != Some(meta.condition().as_str())
        || job.needs != meta.selection.needs(domain)
        || job.cache_mode != Some(CacheMode::Write)
        || job.outputs != source_outputs(meta)
    {
        return Err(invalid("source_producer_scheduling_changed"));
    }
    Ok(())
}

fn no_permissions(permissions: &velnor_actions_contract::Permissions) -> bool {
    [
        permissions.contents,
        permissions.actions,
        permissions.pull_requests,
        permissions.id_token,
        permissions.issues,
        permissions.attestations,
        permissions.pages,
    ]
    .into_iter()
    .all(|level| level == PermissionLevel::None)
}

fn source_outputs(meta: &SourceProducer) -> Vec<JobOutput> {
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

fn validate_consumer_prefix(
    job: &Job,
    meta: &SourceProducer,
    descriptor: &ToolCacheDescriptor,
    setup: &MiseSetup,
) -> Result<usize, RenderError> {
    let expected = crate::tool_producer_steps::tool_consumer_steps(descriptor, setup)?;
    if job.steps.len() < expected.len() {
        return Err(invalid("source_producer_bootstrap_prefix_missing"));
    }
    for (step, canonical) in job.steps.iter().zip(expected.iter()) {
        if step != canonical {
            return Err(invalid("source_producer_bootstrap_prefix_changed"));
        }
    }
    if meta.role == SourceProducerRole::Cargo {
        if !snapshots::matches(
            job.steps.get(4),
            "velnor-rust-source-tools",
            "tools",
            "before",
            None,
        ) {
            return Err(invalid("source_producer_tools_snapshot_changed"));
        }
        Ok(5)
    } else {
        Ok(4)
    }
}

fn validate_evidence(job: &Job, meta: &SourceProducer) -> Result<bool, RenderError> {
    let ids = [
        &meta.restore_step,
        &meta.verification_step,
        &meta.save_step,
        &meta.publication_step,
        &meta.report_step,
    ];
    if ids
        .iter()
        .enumerate()
        .any(|(index, id)| ids[..index].contains(id))
    {
        return Err(invalid("source_producer_duplicate_evidence_binding"));
    }
    let positions = ids.map(|id| {
        job.steps
            .iter()
            .position(|step| step.id.as_ref() == Some(id))
    });
    if positions[4].is_none() {
        return Err(invalid("source_producer_missing_terminal_report"));
    }
    if ids.iter().any(|id| {
        job.steps
            .iter()
            .filter(|step| step.id.as_ref() == Some(id))
            .count()
            != 1
    }) {
        return Err(invalid("source_producer_evidence_binding_changed"));
    }
    if !matches!(positions, [Some(restore), Some(verify), Some(save), Some(publication), Some(report)] if restore < verify && verify < save && save < publication && publication < report && report + 1 == job.steps.len())
    {
        return Err(invalid("source_producer_evidence_order"));
    }
    let save_path = action_path(job, &meta.save_step);
    let publication_path = action_path(job, &meta.publication_step);
    if save_path.is_none() || save_path != publication_path {
        return Err(invalid("source_producer_publication_path_changed"));
    }
    let verification = job
        .steps
        .iter()
        .find(|step| step.id.as_ref() == Some(&meta.verification_step));
    if !verification.is_some_and(|step| {
        matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
            if transport::is_verification(meta.role, invocation.descriptor().operation())
                && env.get("VELNOR_SOURCE_IDENTITY") == Some(&meta.source_identity)
                && step.condition.is_none())
    }) {
        return Err(invalid("source_producer_missing_verification"));
    }
    let report = job
        .steps
        .last()
        .ok_or_else(|| invalid("source_producer_missing_terminal_report"))?;
    if !terminal(report, meta) {
        return Err(invalid("source_producer_missing_terminal_report"));
    }
    Ok(true)
}

fn action_path(job: &Job, id: &velnor_actions_contract::StepId) -> Option<String> {
    job.steps.iter().find_map(|step| {
        (step.id.as_ref() == Some(id)).then(|| match &step.kind {
            StepKind::Action { with, .. } => with.get("path").cloned(),
            _ => None,
        })?
    })
}

fn terminal(step: &Step, meta: &SourceProducer) -> bool {
    let outcome =
        |id: &velnor_actions_contract::StepId| format!("${{{{ steps.{}.outcome }}}}", id.as_str());
    step.id.as_ref() == Some(&meta.report_step)
        && step.condition.as_deref() == Some("always()")
        && matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
            if invocation.descriptor().operation() == SourceBoundOperation::SourceProducerReport
                && report_bindings(meta, env, &outcome))
}

fn report_bindings(
    meta: &SourceProducer,
    env: &BTreeMap<String, String>,
    outcome: &impl Fn(&velnor_actions_contract::StepId) -> String,
) -> bool {
    let output = |id: &velnor_actions_contract::StepId, name: &str| {
        format!("${{{{ steps.{}.outputs.{name} }}}}", id.as_str())
    };
    let (snapshot, changed) = match meta.role {
        SourceProducerRole::Cargo => (
            "${{ steps.velnor-rust-source-after.outcome }}",
            "${{ env.VELNOR_SOURCES_SNAPSHOT_CHANGED }}",
        ),
        SourceProducerRole::Npm => (
            "${{ steps.velnor-npm-source-after.outcome }}",
            "${{ env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED }}",
        ),
        SourceProducerRole::Bun => (
            "${{ steps.velnor-bun-source-after.outcome }}",
            "${{ env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED }}",
        ),
        SourceProducerRole::Tofu | SourceProducerRole::Gradle => ("success", "true"),
    };
    [
        ("VELNOR_SOURCE_IDENTITY", meta.source_identity.clone()),
        ("VELNOR_SOURCE_OUTCOME", outcome(&meta.verification_step)),
        (
            "VELNOR_SOURCE_VERIFIED",
            output(&meta.verification_step, "verified"),
        ),
        (
            "VELNOR_SOURCE_ERROR",
            output(&meta.verification_step, "error"),
        ),
        ("VELNOR_SOURCE_RESTORE_OUTCOME", outcome(&meta.restore_step)),
        (
            "VELNOR_SOURCE_RESTORE_KEY",
            output(&meta.restore_step, "cache-matched-key"),
        ),
        ("VELNOR_SOURCE_SAVE_OUTCOME", outcome(&meta.save_step)),
        (
            "VELNOR_SOURCE_PUBLICATION_OUTCOME",
            outcome(&meta.publication_step),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY",
            output(&meta.publication_step, "cache-matched-key"),
        ),
        ("VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY", meta.save_key()),
        ("VELNOR_SOURCE_SNAPSHOT_OUTCOME", snapshot.to_owned()),
        ("VELNOR_SOURCE_SNAPSHOT_CHANGED", changed.to_owned()),
    ]
    .into_iter()
    .all(|(key, value)| env.get(key).is_some_and(|actual| actual == &value))
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}
