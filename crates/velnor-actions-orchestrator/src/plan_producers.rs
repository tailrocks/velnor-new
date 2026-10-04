//! Producer admission originates only from validated generated workflow roles.
use crate::{OrchestratorError, prepare::GenerationPreparation};
use velnor_actions_contract::{
    ProducerAdmission, ProducerEventContext, ProducerInventory, ProducerPolicy, ProducerRole,
};

pub(crate) fn inventory(
    prep: &GenerationPreparation,
    context: Option<ProducerEventContext>,
) -> Result<ProducerInventory, OrchestratorError> {
    let jobs = crate::finalized::finalized_jobs(prep)?;
    let mut entries = Vec::new();
    for (job_id, job) in jobs {
        let role = match (job.source_producer, job.tool_producer, job.mbx_producer) {
            (Some(producer), None, None) => ProducerRole::Source { producer },
            (None, Some(producer), None) => ProducerRole::Tool { producer },
            (None, None, Some(producer)) => ProducerRole::Mbx { producer },
            (None, None, None) => continue,
            _ => return Err(crate::internal::internal("producer_role_collision")),
        };
        entries.push(ProducerAdmission {
            job_id,
            role,
            policy: ProducerPolicy::AdvisoryFallback,
        });
    }
    Ok(ProducerInventory { context, entries })
}

/// Capture fixed scheduling facts only at the runner-owned request boundary.
pub(crate) fn capture(payload: &serde_json::Value) -> Option<ProducerEventContext> {
    let reference = std::env::var("GITHUB_REF")
        .ok()
        .filter(|value| !value.is_empty())?;
    let default_branch = payload["repository"]["default_branch"].as_str()?.to_owned();
    let protected = match std::env::var("GITHUB_REF_PROTECTED").ok().as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => return None,
    };
    Some(ProducerEventContext {
        reference,
        default_branch,
        protected,
        cargo_fallback: false,
    })
}

/// Observe the actual Plan fallback output through the runner needs channel.
pub(crate) fn capture_merge(
    payload: Option<&str>,
    needs: Option<&str>,
) -> Option<ProducerEventContext> {
    let payload = velnor_actions_contract::parse_strict_json(payload?).ok()?;
    let mut context = capture(&payload)?;
    let needs = velnor_actions_contract::parse_strict_json(needs?).ok()?;
    context.cargo_fallback = match needs["plan"]["outputs"]["cargo_fallback_required"].as_str() {
        Some("true") => true,
        Some("false") => false,
        _ => return None,
    };
    Some(context)
}
