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
    /// Lane-specific runtime cache steps rendered after checkout and before each shared call.
    pub preludes: BTreeMap<String, Vec<Step>>,
    /// Composite action files, one per logical job.
    pub files: Vec<RenderedFile>,
}

struct SharedLaneParts {
    checkout: Step,
    common: Vec<Step>,
    hosted_prelude: Vec<Step>,
    local_prelude: Vec<Step>,
    hosted_extra: Vec<Step>,
    local_extra: Vec<Step>,
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
    let mut preludes = BTreeMap::new();
    let mut files = Vec::new();
    let mut next = jobs.clone();
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
        preludes.insert(hosted_id.clone(), parts.hosted_prelude);
        preludes.insert(local_id.clone(), parts.local_prelude);
        set_steps(&mut next, &hosted_id, parts.hosted_extra);
        set_steps(&mut next, &local_id, parts.local_extra);
    }
    Ok(LaneShare {
        jobs: next,
        calls,
        checkouts,
        preludes,
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
    let (hosted_prelude, hosted_steps) = peel_tools_cache_prelude(hosted_steps)?;
    let (local_prelude, local_steps) = peel_tools_cache_prelude(local_steps)?;
    if !same_prelude_shape(&hosted_prelude, &local_prelude) {
        return None;
    }
    let (hosted_common, hosted_extra) = peel_saves(&hosted_steps);
    let (local_common, local_extra) = peel_saves(&local_steps);
    (hosted_common == local_common).then_some(SharedLaneParts {
        checkout: hosted_checkout.clone(),
        common: hosted_common,
        hosted_prelude,
        local_prelude,
        hosted_extra,
        local_extra,
    })
}

/// Keep runner-specific V2 identity and restore steps outside shared actions.
fn peel_tools_cache_prelude(steps: &[Step]) -> Option<(Vec<Step>, Vec<Step>)> {
    let mut prelude = Vec::new();
    let mut common = Vec::new();
    for step in steps {
        if matches!(
            step.name.as_str(),
            crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME | crate::cache_steps::TOOLS_RESTORE_NAME
        ) {
            prelude.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    if !valid_tools_cache_prelude(&prelude) {
        return None;
    }
    if let Some(setup) = steps
        .iter()
        .position(|step| step.name == crate::setup::SETUP_MISE_NAME)
        && steps.iter().enumerate().any(|(index, step)| {
            matches!(
                step.name.as_str(),
                crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME
                    | crate::cache_steps::TOOLS_RESTORE_NAME
            ) && index > setup
        })
    {
        return None;
    }
    Some((prelude, common))
}

fn valid_tools_cache_prelude(steps: &[Step]) -> bool {
    match steps {
        [] => true,
        [identity, restore] => {
            let identity_ok = identity.name == crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME
                && identity.condition.is_none()
                && matches!(&identity.kind, StepKind::Shell { .. });
            let expected_paths = crate::cache_steps::TOOLS_CACHE_PATHS.join("\n");
            let restore_ok = matches!(
                &restore.kind,
                StepKind::Action { uses, with, env }
                    if restore.name == crate::cache_steps::TOOLS_RESTORE_NAME
                        && uses == crate::cache_steps::TOOLS_RESTORE_USES
                        && env.is_empty()
                        && with.get("path").map(String::as_str) == Some(expected_paths.as_str())
                        && with.get("key").is_some_and(|key| {
                            key.starts_with("mise-tools-v2-")
                                && key.contains("${{steps.velnor-tool-cache-identity.outputs.identity}}")
                        })
            );
            identity_ok
                && restore_ok
                && restore.condition.as_deref()
                    == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
        }
        _ => false,
    }
}

fn same_prelude_shape(hosted: &[Step], local: &[Step]) -> bool {
    if !valid_tools_cache_prelude(hosted)
        || !valid_tools_cache_prelude(local)
        || hosted.len() != local.len()
    {
        return false;
    }
    hosted.iter().zip(local).all(|(hosted, local)| {
        if hosted.name != local.name || hosted.condition != local.condition {
            return false;
        }
        match (&hosted.kind, &local.kind) {
            (
                StepKind::Shell {
                    run: hosted_run,
                    env: hosted_env,
                },
                StepKind::Shell {
                    run: local_run,
                    env: local_env,
                },
            ) => hosted_run == local_run && hosted_env.keys().eq(local_env.keys()),
            (
                StepKind::Action {
                    uses: hosted_uses,
                    with: hosted_with,
                    env: hosted_env,
                },
                StepKind::Action {
                    uses: local_uses,
                    with: local_with,
                    env: local_env,
                },
            ) => {
                hosted_uses == local_uses
                    && hosted_env == local_env
                    && hosted_with.keys().eq(local_with.keys())
                    && hosted_with.get("path") == local_with.get("path")
                    && hosted_with.get("restore-keys") == local_with.get("restore-keys")
            }
            _ => false,
        }
    })
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

fn peel_saves(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut extra = Vec::new();
    for step in steps {
        if is_elected_save(step) {
            extra.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, extra)
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
        rendered.push(step_to_yaml(logical, step, ctx, &[], true, &empty_job_env)?);
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
#[path = "lane_share_shell_tests.rs"]
mod shell_tests;

#[cfg(test)]
#[path = "lane_share_unpinned_tests.rs"]
mod unpinned_tests;
