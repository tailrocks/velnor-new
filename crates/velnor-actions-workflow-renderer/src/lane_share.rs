//! Composite actions for common steps in duplicated verification lanes.
//!
//! GitHub will not start a workflow file larger than 500 KB. Inlining both
//! lane step lists crosses that limit, so common step runs live in actions
//! while each lane job keeps its own id, `runs-on`, and `needs`.

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
    /// Composite actions for common step runs in paired lanes.
    pub shared: Vec<RenderedFile>,
}

/// Jobs rewritten to call a shared action, plus those action files.
#[derive(Debug)]
pub(crate) struct LaneShare {
<<<<<<< HEAD
    /// Same jobs, with shared steps retained for document-boundary indexing.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to ordered calls; each call replaces the indicated source steps.
    pub calls: BTreeMap<String, Vec<SharedCall>>,
    /// Composite action files, one per shared step run.
    pub files: Vec<RenderedFile>,
}

/// A composite call inserted before `before_step`, replacing `skip_steps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedCall {
    pub before_step: usize,
    pub skip_steps: usize,
    pub uses: String,
}

/// Factor common runs from `__hosted` / `__local` pairs.
=======
    /// Same jobs, with shared lanes keeping their headers and elected saves.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to local `uses` path.
    pub calls: BTreeMap<String, String>,
    /// Job id to the original checkout step rendered before each shared call.
    pub checkouts: BTreeMap<String, Step>,
    /// Composite action files, one per logical job.
    pub files: Vec<RenderedFile>,
}

struct SharedLaneParts {
    checkout: Step,
    common: Vec<Step>,
    hosted_extra: Vec<Step>,
    local_extra: Vec<Step>,
}

/// Factor `__hosted` / `__local` pairs whose step lists match.
>>>>>>> origin/main
///
/// # Errors
///
/// A non-MBX pair whose timeout, condition, permissions, environment, or
/// steps differ fails closed. MBX actions and elected cache saves stay in
/// their owning jobs; common runs on either side of them are factored as
/// separate composites. An unsafe logical id fails closed.
pub(crate) fn share_lanes(
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<LaneShare, RenderError> {
    let mut calls = BTreeMap::new();
    let mut checkouts = BTreeMap::new();
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
<<<<<<< HEAD
        // MBX inputs are lane-specific after hosted-cache isolation. Keep
        // every MBX action in its job, then factor exact common runs in
        // each lane's original order.
        if hosted.steps.iter().any(crate::cache_steps::is_mbx_action)
            || local.steps.iter().any(crate::cache_steps::is_mbx_action)
        {
            if !same_job_policy(hosted, local) {
                continue;
            }
            let (hosted_shared, local_shared) =
                share_mbx_runs(logical, hosted, local, ctx, &mut files)?;
            if !hosted_shared.is_empty() {
                calls.insert(hosted_id, hosted_shared);
            }
            if !local_shared.is_empty() {
                calls.insert(local_id, local_shared);
            }
            continue;
        }
        let Some((common, hosted_extra, local_extra)) = split_pair(hosted, local) else {
=======
        let Some(parts) = split_pair(hosted, local, &ctx.checkout_uses) else {
>>>>>>> origin/main
            return Err(RenderError::InvalidWorkflow(format!(
                "lane_body_differs:{logical}"
            )));
        };
<<<<<<< HEAD
        let uses = format!("$/.github/actions/{logical}");
        files.push(composite_file(logical, &common, ctx)?);
        let call = SharedCall {
            before_step: 0,
            skip_steps: 0,
            uses,
        };
        calls.insert(hosted_id.clone(), vec![call.clone()]);
        calls.insert(local_id.clone(), vec![call]);
        set_steps(&mut next, &hosted_id, hosted_extra);
        set_steps(&mut next, &local_id, local_extra);
=======
        let uses = format!("./.github/actions/{logical}");
        files.push(composite_file(logical, &parts.common, ctx)?);
        calls.insert(hosted_id.clone(), uses.clone());
        calls.insert(local_id.clone(), uses);
        checkouts.insert(hosted_id.clone(), parts.checkout.clone());
        checkouts.insert(local_id.clone(), parts.checkout);
        set_steps(&mut next, &hosted_id, parts.hosted_extra);
        set_steps(&mut next, &local_id, parts.local_extra);
>>>>>>> origin/main
    }
    Ok(LaneShare {
        jobs: next,
        calls,
        checkouts,
        files,
    })
}

fn same_job_policy(hosted: &Job, local: &Job) -> bool {
    hosted.timeout_minutes == local.timeout_minutes
        && hosted.condition == local.condition
        && hosted.permissions == local.permissions
        && hosted.environment == local.environment
}

/// Factor exact common runs from an ordered LCS. Any MBX or elected save
/// step remains job-local, even when lane boundaries differ.
fn share_mbx_runs(
    logical: &str,
    hosted: &Job,
    local: &Job,
    ctx: &RenderContext,
    files: &mut Vec<RenderedFile>,
) -> Result<(Vec<SharedCall>, Vec<SharedCall>), RenderError> {
    let matches = common_step_matches(hosted, local);
    let mut writer = SharedRunWriter::new(logical, ctx, files);
    let mut current_start = None;
    let mut previous = None;
    let mut common = Vec::new();
    for (hosted_index, local_index) in matches {
        if previous.is_some_and(|(previous_hosted, previous_local)| {
            hosted_index != previous_hosted + 1 || local_index != previous_local + 1
        }) {
            let start = current_start.take().ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("lane_lcs_missing_run_start:{logical}"))
            })?;
            writer.append(start, &common)?;
            common.clear();
        }
        current_start.get_or_insert((hosted_index, local_index));
        common.push(hosted.steps[hosted_index].clone());
        previous = Some((hosted_index, local_index));
    }
    if let Some((hosted_start, local_start)) = current_start {
        writer.append((hosted_start, local_start), &common)?;
    }
    Ok(writer.finish())
}

fn common_step_matches(hosted: &Job, local: &Job) -> Vec<(usize, usize)> {
    let hosted_steps: Vec<(usize, &Step)> = hosted
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| !crate::cache_steps::is_mbx_action(step) && !is_elected_save(step))
        .collect();
    let local_steps: Vec<(usize, &Step)> = local
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| !crate::cache_steps::is_mbx_action(step) && !is_elected_save(step))
        .collect();
    crate::lane_share_lcs::ordered_matches(&hosted_steps, &local_steps)
}

struct SharedRunWriter<'a> {
    logical: &'a str,
    ctx: &'a RenderContext,
    files: &'a mut Vec<RenderedFile>,
    segment: usize,
    hosted_calls: Vec<SharedCall>,
    local_calls: Vec<SharedCall>,
}

impl<'a> SharedRunWriter<'a> {
    fn new(logical: &'a str, ctx: &'a RenderContext, files: &'a mut Vec<RenderedFile>) -> Self {
        Self {
            logical,
            ctx,
            files,
            segment: 0,
            hosted_calls: Vec::new(),
            local_calls: Vec::new(),
        }
    }

    fn append(
        &mut self,
        (hosted_start, local_start): (usize, usize),
        common: &[Step],
    ) -> Result<(), RenderError> {
        let file_name = format!("{}-shared-{}", self.logical, self.segment);
        let action_path = format!(
            ".github/actions/shared-lanes/{}/segment-{}",
            self.logical, self.segment
        );
        let uses = format!("$/{action_path}");
        let file = composite_file_at(&file_name, &action_path, common, self.ctx)?;
        let skip_steps = common.len();
        self.hosted_calls.push(SharedCall {
            before_step: hosted_start,
            skip_steps,
            uses: uses.clone(),
        });
        self.local_calls.push(SharedCall {
            before_step: local_start,
            skip_steps,
            uses,
        });
        self.files.push(file);
        self.segment += 1;
        Ok(())
    }

    fn finish(self) -> (Vec<SharedCall>, Vec<SharedCall>) {
        (self.hosted_calls, self.local_calls)
    }
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

<<<<<<< HEAD
fn split_pair(hosted: &Job, local: &Job) -> Option<(Vec<Step>, Vec<Step>, Vec<Step>)> {
    if !same_job_policy(hosted, local) {
=======
fn split_pair(hosted: &Job, local: &Job, checkout_uses: &str) -> Option<SharedLaneParts> {
    if hosted.timeout_minutes != local.timeout_minutes
        || hosted.condition != local.condition
        || hosted.permissions != local.permissions
        || hosted.environment != local.environment
    {
>>>>>>> origin/main
        return None;
    }
    let (hosted_checkout, hosted_steps) = peel_checkout(&hosted.steps, checkout_uses)?;
    let (local_checkout, local_steps) = peel_checkout(&local.steps, checkout_uses)?;
    if hosted_checkout != local_checkout {
        return None;
    }
    let (hosted_common, hosted_extra) = peel_saves(hosted_steps);
    let (local_common, local_extra) = peel_saves(local_steps);
    (hosted_common == local_common).then_some(SharedLaneParts {
        checkout: hosted_checkout.clone(),
        common: hosted_common,
        hosted_extra,
        local_extra,
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
    composite_file_at(logical, &format!(".github/actions/{logical}"), steps, ctx)
}

fn composite_file_at(
    logical: &str,
    action_path: &str,
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
        path: format!("{action_path}/action.yml"),
        bytes,
    })
}

#[cfg(test)]
#[path = "lane_share_tests.rs"]
mod tests;
<<<<<<< HEAD
=======

#[cfg(test)]
#[path = "lane_share_shell_tests.rs"]
mod shell_tests;

#[cfg(test)]
#[path = "lane_share_unpinned_tests.rs"]
mod unpinned_tests;
>>>>>>> origin/main
