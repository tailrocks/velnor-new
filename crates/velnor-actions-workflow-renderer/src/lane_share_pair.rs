//! Validation and decomposition of hosted/local lane pairs.

use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::workflow::lanes::{
    HOSTED_SUFFIX, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

/// Source steps separated into lane prefix, prelude, shared body, and postludes.
pub(super) struct SharedLaneParts {
    pub(super) checkout: Step,
    pub(super) prefix: Vec<Step>,
    pub(super) hosted_prelude: Vec<Step>,
    pub(super) local_prelude: Vec<Step>,
    pub(super) common: Vec<Step>,
    pub(super) hosted_postlude: Vec<Step>,
    pub(super) local_postlude: Vec<Step>,
}

struct MbxSections<'a> {
    prefix: Vec<Step>,
    hosted_prelude: Vec<Step>,
    local_prelude: Vec<Step>,
    hosted_tail: &'a [Step],
    local_tail: &'a [Step],
}

pub(super) fn hosted_ids(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    jobs.keys()
        .filter(|id| id.ends_with(HOSTED_SUFFIX))
        .cloned()
        .collect()
}

pub(super) fn logical_id(hosted_id: &str) -> Option<&str> {
    let logical = hosted_id.strip_suffix(HOSTED_SUFFIX)?;
    let ok = !logical.is_empty()
        && logical
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    ok.then_some(logical)
}

pub(super) fn split_pair(
    hosted: &Job,
    local: &Job,
    checkout_uses: &str,
) -> Option<SharedLaneParts> {
    if hosted.timeout_minutes != local.timeout_minutes
        || !same_or_admitted_check_condition(hosted, local)
        || hosted.permissions != local.permissions
        || hosted.environment != local.environment
    {
        return None;
    }
    let (hosted_checkout, hosted_steps) = peel_checkout(&hosted.steps, checkout_uses)?;
    let (local_checkout, local_steps) = peel_checkout(&local.steps, checkout_uses)?;
    if hosted_checkout != local_checkout {
        return None;
    }
    split_shared_steps(hosted_checkout, hosted_steps, local_steps)
}

fn split_shared_steps(
    checkout: &Step,
    hosted_steps: &[Step],
    local_steps: &[Step],
) -> Option<SharedLaneParts> {
    let hosted_action = hosted_steps.iter().any(crate::cache_steps::is_mbx_action);
    let local_action = local_steps.iter().any(crate::cache_steps::is_mbx_action);
    if hosted_action != local_action {
        return None;
    }
    let MbxSections {
        prefix,
        mut hosted_prelude,
        mut local_prelude,
        hosted_tail,
        local_tail,
    } = split_mbx_sections(hosted_steps, local_steps, hosted_action)?;
    let (hosted_provider_prefix, hosted_tail) =
        crate::lane_share_sections::peel_provider_restore_prefix(hosted_tail);
    let (local_provider_prefix, local_tail) =
        crate::lane_share_sections::peel_provider_restore_prefix(local_tail);
    if hosted_provider_prefix != local_provider_prefix {
        return None;
    }
    hosted_prelude.extend(hosted_provider_prefix);
    local_prelude.extend(local_provider_prefix);
    let (hosted_common, hosted_cache_postlude) =
        crate::lane_share_sections::peel_postlude(hosted_tail);
    let (local_common, local_cache_postlude) =
        crate::lane_share_sections::peel_postlude(local_tail);
    let (hosted_common, hosted_lane_specific) = peel_lane_specific(&hosted_common);
    let (local_common, local_lane_specific) = peel_lane_specific(&local_common);
    let mut hosted_postlude = hosted_lane_specific;
    hosted_postlude.extend(hosted_cache_postlude);
    let mut local_postlude = local_lane_specific;
    local_postlude.extend(local_cache_postlude);
    (hosted_common == local_common).then_some(SharedLaneParts {
        checkout: checkout.clone(),
        prefix,
        hosted_prelude,
        local_prelude,
        common: hosted_common,
        hosted_postlude,
        local_postlude,
    })
}

fn split_mbx_sections<'a>(
    hosted_steps: &'a [Step],
    local_steps: &'a [Step],
    has_mbx_action: bool,
) -> Option<MbxSections<'a>> {
    if !has_mbx_action {
        return Some(MbxSections {
            prefix: Vec::new(),
            hosted_prelude: Vec::new(),
            local_prelude: Vec::new(),
            hosted_tail: hosted_steps,
            local_tail: local_steps,
        });
    }
    let hosted_at = crate::lane_share_sections::mbx_prelude_index(hosted_steps)?;
    let local_at = crate::lane_share_sections::mbx_prelude_index(local_steps)?;
    let hosted_prefix = &hosted_steps[..hosted_at];
    let local_prefix = &local_steps[..local_at];
    if hosted_prefix != local_prefix {
        return None;
    }
    let (hosted_prelude, hosted_tail) =
        crate::lane_share_sections::peel_mbx_prelude(&hosted_steps[hosted_at..])?;
    let (local_prelude, local_tail) =
        crate::lane_share_sections::peel_mbx_prelude(&local_steps[local_at..])?;
    Some(MbxSections {
        prefix: hosted_prefix.to_vec(),
        hosted_prelude,
        local_prelude,
        hosted_tail,
        local_tail,
    })
}

fn same_or_admitted_check_condition(hosted: &Job, local: &Job) -> bool {
    if hosted.condition == local.condition {
        return true;
    }
    hosted.condition.is_none()
        && local.condition.as_deref() == Some(EPHEMERAL_CHECK_ADMISSION_CONDITION)
        && matches!(
            (&hosted.check_runner, &local.check_runner),
            (Some(hosted_runner), Some(local_runner))
                if hosted_runner == local_runner
                    && hosted_runner.platform == CheckPlatform::LinuxX64
                    && hosted_runner.executor == CheckExecutor::Hosted
        )
}

fn peel_checkout<'a>(steps: &'a [Step], checkout_uses: &str) -> Option<(&'a Step, &'a [Step])> {
    let (checkout, remaining) = steps.split_first()?;
    let is_expected_checkout = checkout.role == Some(StepRole::Checkout)
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, env }
                if uses == checkout_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        );
    if !is_expected_checkout || remaining.iter().any(is_checkout_step) {
        return None;
    }
    Some((checkout, remaining))
}

fn is_checkout_step(step: &Step) -> bool {
    step.role == Some(StepRole::Checkout)
        || matches!(
            &step.kind,
            StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@")
        )
}

fn peel_lane_specific(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut extra = Vec::new();
    for step in steps {
        if is_lane_specific(step) {
            extra.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, extra)
}

fn is_lane_specific(step: &Step) -> bool {
    if is_elected_save(step) {
        return true;
    }
    match &step.kind {
        StepKind::Action { uses, .. } if uses == crate::steps::UPLOAD_ARTIFACT_USES => true,
        StepKind::Shell { env, .. } | StepKind::Internal { env, .. } => {
            env.contains_key(NAMED_CHECK_JOB_ID_ENV)
                || env.contains_key(NAMED_CHECK_LANE_VARIANT_ENV)
        }
        StepKind::Action { .. } => false,
    }
}

fn is_elected_save(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave)
    )
}

pub(super) fn set_steps(jobs: &mut BTreeMap<String, Job>, id: &str, steps: Vec<Step>) {
    if let Some(job) = jobs.get_mut(id) {
        job.steps = steps;
    }
}
