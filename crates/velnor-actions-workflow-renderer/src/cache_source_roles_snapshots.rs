//! Exact native source snapshot observers and ordering.
use crate::RenderError;
use velnor_actions_contract::{
    CacheSnapshotDomain, Job, SourceBoundOperation, SourceProducer, SourceProducerRole, Step,
    StepKind,
};

pub(super) fn validate(
    job: &Job,
    meta: &SourceProducer,
    prefix_len: usize,
) -> Result<(), RenderError> {
    let expected = observations(meta);
    let snapshots: Vec<_> = job
        .steps
        .iter()
        .filter(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::CacheSnapshot)
        })
        .collect();
    if snapshots.len() != expected.len() {
        return Err(invalid("source_producer_snapshot_count_changed"));
    }
    for (id, layer, phase, condition) in &expected {
        let matches: Vec<_> = job
            .steps
            .iter()
            .filter(|step| step.id.as_ref().is_some_and(|value| value.as_str() == *id))
            .collect();
        if matches.len() != 1
            || !self::matches(
                matches.first().copied(),
                id,
                layer,
                phase,
                condition.as_deref(),
            )
        {
            return Err(invalid("source_producer_snapshot_binding_changed"));
        }
    }
    let restore = job
        .steps
        .iter()
        .position(|step| step.id == Some(meta.restore_step.clone()));
    let verification = job
        .steps
        .iter()
        .position(|step| step.id == Some(meta.verification_step.clone()));
    let save = job
        .steps
        .iter()
        .position(|step| step.id == Some(meta.save_step.clone()));
    for (id, _, phase, _) in &expected {
        if *id == "velnor-rust-source-tools" {
            continue;
        }
        let at = job
            .steps
            .iter()
            .position(|step| step.id.as_ref().is_some_and(|value| value.as_str() == *id));
        if *phase == "before"
            && !matches!((at, restore, verification), (Some(a), Some(r), Some(v)) if r < a && a < v)
            || *phase == "after"
                && !matches!((at, verification, save), (Some(a), Some(v), Some(s)) if v < a && a < s)
        {
            return Err(invalid("source_producer_snapshot_order_changed"));
        }
    }
    if meta.role == SourceProducerRole::Cargo && prefix_len != 5 {
        return Err(invalid("source_producer_tools_snapshot_changed"));
    }
    Ok(())
}

type Observation = (&'static str, &'static str, &'static str, Option<String>);

fn observations(meta: &SourceProducer) -> Vec<Observation> {
    match meta.role {
        SourceProducerRole::Cargo => vec![
            ("velnor-rust-source-tools", "tools", "before", None),
            ("velnor-rust-source-before", "sources", "before", None),
            ("velnor-rust-source-after", "sources", "after", None),
        ],
        SourceProducerRole::Npm => vec![
            ("velnor-npm-source-before", "npm_downloads", "before", None),
            (
                "velnor-npm-source-after",
                "npm_downloads",
                "after",
                Some(format!(
                    "steps.{}.outputs.verified == 'true'",
                    meta.verification_step.as_str()
                )),
            ),
        ],
        SourceProducerRole::Bun => vec![
            ("velnor-bun-source-before", "bun_downloads", "before", None),
            (
                "velnor-bun-source-after",
                "bun_downloads",
                "after",
                Some(format!(
                    "steps.{}.outputs.verified == 'true'",
                    meta.verification_step.as_str()
                )),
            ),
        ],
        SourceProducerRole::Tofu | SourceProducerRole::Gradle => Vec::new(),
    }
}

pub(super) fn matches(
    step: Option<&Step>,
    id: &str,
    layer: &str,
    phase: &str,
    condition: Option<&str>,
) -> bool {
    let Some(Step {
        id: Some(step_id),
        condition: actual_condition,
        kind: StepKind::SourceBoundHelper { invocation, env },
        ..
    }) = step
    else {
        return false;
    };
    step_id.as_str() == id
        && invocation.descriptor().operation() == SourceBoundOperation::CacheSnapshot
        && invocation.args() == [layer.to_owned(), phase.to_owned()]
        && env
            .get("VELNOR_SNAPSHOT_LAYER")
            .is_some_and(|value| value == layer)
        && env
            .get("VELNOR_SNAPSHOT_PHASE")
            .is_some_and(|value| value == phase)
        && CacheSnapshotDomain::ALL
            .into_iter()
            .find(|domain| domain.name() == layer)
            .is_some_and(|domain| *env == domain.environment(phase == "before"))
        && actual_condition.as_deref() == condition
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}
