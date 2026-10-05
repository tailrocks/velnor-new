//! One composite action per duplicated verification lane.
//!
//! GitHub will not start a workflow file larger than 500 KB. Inlining both
//! lane step lists crosses that limit, so the steps live in one action and
//! each lane job keeps its own id, `runs-on`, and `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, Step, StepKind};

use crate::composite::composite_yaml;
use crate::document_steps::step_to_yaml;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, yaml::render_yaml};

/// CI workflow plus composite actions for duplicated lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedWorkflow {
    /// Marked `ci.yml` bytes.
    pub yaml: String,
    /// One composite action per shared logical job. Empty when no lane pair exists.
    pub shared: Vec<RenderedFile>,
}

/// Jobs rewritten to call a shared action, plus those action files.
#[derive(Debug)]
pub(crate) struct LaneShare {
    /// Same jobs, with shared lanes keeping their headers and elected saves.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to local `uses` path.
    pub calls: BTreeMap<String, String>,
    /// Job id to the original checkout step rendered before each shared call.
    pub checkouts: BTreeMap<String, Step>,
    /// Original step lists used to derive job-level environment values.
    pub env_steps: BTreeMap<String, Vec<Step>>,
    /// Common setup steps rendered before each lane's cache-specific prelude.
    pub prefixes: BTreeMap<String, Vec<Step>>,
    /// Lane-specific MBX setup, restore, and import steps.
    pub preludes: BTreeMap<String, Vec<Step>>,
    /// Lane-specific cache exports and saves rendered after the shared call.
    pub postludes: BTreeMap<String, Vec<Step>>,
    /// Composite action files, one per logical job.
    pub files: Vec<RenderedFile>,
}

struct SharedLaneParts {
    checkout: Step,
    prefix: Vec<Step>,
    hosted_prelude: Vec<Step>,
    local_prelude: Vec<Step>,
    common: Vec<Step>,
    hosted_postlude: Vec<Step>,
    local_postlude: Vec<Step>,
}

/// Factor `__hosted` / `__local` pairs whose step lists match.
///
/// # Errors
///
/// A pair whose timeout, condition, permissions, environment, or steps
/// differ fails closed. Elected cache saves (`Save Mise tools`, `Save Tofu
/// providers`) stay on the job that owns them and are not part of that
/// comparison. An unsafe logical id fails closed.
pub(crate) fn share_lanes(
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<LaneShare, RenderError> {
    let mut calls = BTreeMap::new();
    let mut checkouts = BTreeMap::new();
    let mut prefixes = BTreeMap::new();
    let mut preludes = BTreeMap::new();
    let mut postludes = BTreeMap::new();
    let mut files = Vec::new();
    let mut next = jobs.clone();
    let mut env_steps = BTreeMap::new();
    for hosted_id in hosted_ids(jobs) {
        let Some(logical) = logical_id(&hosted_id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_lane_id:{hosted_id}"
            )));
        };
        let local_id = format!("{logical}{SCALE_SUFFIX}");
        let Some(local) = jobs.get(&local_id) else {
            continue;
        };
        let Some(hosted) = jobs.get(&hosted_id) else {
            continue;
        };
        let Some(parts) = split_pair(hosted, local, &ctx.checkout_uses) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "lane_body_differs:{logical}"
            )));
        };
        let uses = format!("./.github/actions/{logical}");
        files.push(composite_file(logical, &parts.common, ctx)?);
        calls.insert(hosted_id.clone(), uses.clone());
        calls.insert(local_id.clone(), uses);
        checkouts.insert(hosted_id.clone(), parts.checkout.clone());
        checkouts.insert(local_id.clone(), parts.checkout);
        prefixes.insert(hosted_id.clone(), parts.prefix.clone());
        prefixes.insert(local_id.clone(), parts.prefix);
        preludes.insert(hosted_id.clone(), parts.hosted_prelude);
        preludes.insert(local_id.clone(), parts.local_prelude);
        env_steps.insert(hosted_id.clone(), hosted.steps.clone());
        env_steps.insert(local_id.clone(), local.steps.clone());
        set_steps(&mut next, &hosted_id, parts.hosted_postlude.clone());
        set_steps(&mut next, &local_id, parts.local_postlude.clone());
        postludes.insert(hosted_id.clone(), parts.hosted_postlude);
        postludes.insert(local_id.clone(), parts.local_postlude);
    }
    Ok(LaneShare {
        jobs: next,
        calls,
        checkouts,
        env_steps,
        prefixes,
        preludes,
        postludes,
        files,
    })
}

fn hosted_ids(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    jobs.keys()
        .filter(|id| id.ends_with(HOSTED_SUFFIX))
        .cloned()
        .collect()
}

fn logical_id(hosted_id: &str) -> Option<&str> {
    let logical = hosted_id.strip_suffix(HOSTED_SUFFIX)?;
    let ok = !logical.is_empty()
        && logical
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    ok.then_some(logical)
}

fn split_pair(hosted: &Job, local: &Job, checkout_uses: &str) -> Option<SharedLaneParts> {
    if hosted.timeout_minutes != local.timeout_minutes
        || hosted.condition != local.condition
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
    let (prefix, hosted_prelude, local_prelude, hosted_tail, local_tail) = if hosted_action {
        let hosted_at = mbx_prelude_index(hosted_steps)?;
        let local_at = mbx_prelude_index(local_steps)?;
        let hosted_prefix = &hosted_steps[..hosted_at];
        let local_prefix = &local_steps[..local_at];
        if hosted_prefix != local_prefix {
            return None;
        }
        let (hosted_prelude, hosted_tail) = peel_mbx_prelude(&hosted_steps[hosted_at..])?;
        let (local_prelude, local_tail) = peel_mbx_prelude(&local_steps[local_at..])?;
        (
            hosted_prefix.to_vec(),
            hosted_prelude,
            local_prelude,
            hosted_tail,
            local_tail,
        )
    } else {
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            hosted_steps,
            local_steps,
        )
    };
    let (hosted_common, hosted_postlude) = peel_postlude(hosted_tail);
    let (local_common, local_postlude) = peel_postlude(local_tail);
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

fn mbx_prelude_index(steps: &[Step]) -> Option<usize> {
    let preflight = steps
        .iter()
        .position(|step| step.name == crate::cache_steps::MBX_PREFLIGHT_NAME)?;
    (preflight < steps.len()).then_some(preflight)
}

fn peel_mbx_prelude(steps: &[Step]) -> Option<(Vec<Step>, &[Step])> {
    let mut end = 0;
    for step in steps {
        if is_mbx_prelude_step(step) {
            end += 1;
        } else {
            break;
        }
    }
    let prelude = &steps[..end];
    if prelude.first()?.name != crate::cache_steps::MBX_PREFLIGHT_NAME
        || !prelude.iter().any(crate::cache_steps::is_mbx_action)
    {
        return None;
    }
    Some((prelude.to_vec(), &steps[end..]))
}

fn is_mbx_prelude_step(step: &Step) -> bool {
    matches!(
        step.name.as_str(),
        crate::cache_steps::MBX_PREFLIGHT_NAME
            | crate::cache_steps::MBX_RESTORE_NAME
            | crate::cache_steps::MBX_VERSION_CHECK_NAME
    )
}

fn peel_postlude(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut postlude = Vec::new();
    for step in steps {
        if is_postlude_step(step) {
            postlude.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, postlude)
}

fn is_postlude_step(step: &Step) -> bool {
    is_elected_save(step)
}

fn peel_checkout<'a>(steps: &'a [Step], checkout_uses: &str) -> Option<(&'a Step, &'a [Step])> {
    let (checkout, remaining) = steps.split_first()?;
    let is_expected_checkout = checkout.name == "Checkout"
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
    step.name == "Checkout"
        || matches!(
            &step.kind,
            StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@")
        )
}

fn is_elected_save(step: &Step) -> bool {
    step.name == crate::cache_steps::TOOLS_SAVE_NAME
        || step.name == crate::tofu_cache::TOFU_PROVIDERS_SAVE_NAME
}

fn set_steps(jobs: &mut BTreeMap<String, Job>, id: &str, steps: Vec<Step>) {
    if let Some(job) = jobs.get_mut(id) {
        job.steps = steps;
    }
}

fn composite_file(
    logical: &str,
    steps: &[Step],
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    let mut rendered = Vec::with_capacity(steps.len());
    let empty_job_env = BTreeMap::new();
    for step in steps {
        rendered.push(step_to_yaml(
            logical,
            step,
            ctx,
            &[],
            true,
            &empty_job_env,
            false,
        )?);
    }
    let body = composite_yaml(logical, rendered)?;
    let quoted = crate::yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(&ctx.generator_version, &render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{logical}/action.yml"),
        bytes,
    })
}

#[cfg(test)]
#[path = "lane_share_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lane_share_render_tests.rs"]
mod render_tests;

#[cfg(test)]
#[path = "lane_share_shell_tests.rs"]
mod shell_tests;

#[cfg(test)]
#[path = "lane_share_unpinned_tests.rs"]
mod unpinned_tests;
