//! One composite action per duplicated verification lane.
//!
//! GitHub will not start a workflow file larger than 500 KB. Inlining both
//! lane step lists crosses that limit, so the steps live in one action and
//! each lane job keeps its own id, `runs-on`, and `needs`.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, PullRequestCachePolicy, Step};

use crate::composite::composite_yaml;
use crate::document::step_to_yaml;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, yaml::render_yaml};

const MBX_SHARED_CALL_ID: &str = "mbx-lane-cache";

/// One local composite call, with an optional stable output handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedCall {
    pub uses: String,
    pub id: Option<String>,
}

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
    /// Same jobs, with shared lanes keeping their headers and empty steps.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to local `uses` path.
    pub calls: BTreeMap<String, SharedCall>,
    /// Composite action files, one per logical job.
    pub files: Vec<RenderedFile>,
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
        let Some((common, mut hosted_extra, mut local_extra)) = split_pair(hosted, local) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "lane_body_differs:{logical}"
            )));
        };
        let uses = format!("$/.github/actions/{logical}");
        let has_mbx_cache = common
            .iter()
            .any(|step| step.name == crate::mbx_bundle::MBX_BUNDLE_RESTORE_NAME);
        let id = has_mbx_cache.then(|| MBX_SHARED_CALL_ID.to_owned());
        let cache_policy = if has_mbx_cache {
            crate::mbx_bundle::shared_lane_policy(&common, ctx.pull_request_cache_policy)?
        } else {
            ctx.pull_request_cache_policy
        };
        if has_mbx_cache {
            crate::mbx_bundle::bind_shared_lane_outputs(&mut hosted_extra, cache_policy)?;
            crate::mbx_bundle::bind_shared_lane_outputs(&mut local_extra, cache_policy)?;
        }
        files.push(composite_file(
            logical,
            &common,
            ctx,
            has_mbx_cache,
            cache_policy,
        )?);
        let call = SharedCall { uses, id };
        calls.insert(hosted_id.clone(), call.clone());
        calls.insert(local_id.clone(), call);
        set_steps(&mut next, &hosted_id, hosted_extra);
        set_steps(&mut next, &local_id, local_extra);
    }
    Ok(LaneShare {
        jobs: next,
        calls,
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

fn split_pair(hosted: &Job, local: &Job) -> Option<(Vec<Step>, Vec<Step>, Vec<Step>)> {
    if hosted.timeout_minutes != local.timeout_minutes
        || hosted.condition != local.condition
        || hosted.permissions != local.permissions
        || hosted.environment != local.environment
    {
        return None;
    }
    let (hosted_common, hosted_extra) = peel_saves(&hosted.steps);
    let (local_common, local_extra) = peel_saves(&local.steps);
    (hosted_common == local_common).then_some((hosted_common, hosted_extra, local_extra))
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
        || step.name == crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME
        || step.name == crate::mbx_bundle::MBX_BUNDLE_SAVE_NAME
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
    has_mbx_cache: bool,
    cache_policy: PullRequestCachePolicy,
) -> Result<RenderedFile, RenderError> {
    let mut rendered = Vec::with_capacity(steps.len());
    for step in steps {
        rendered.push(step_to_yaml(logical, step, ctx, &[], true)?);
    }
    let outputs = has_mbx_cache.then(|| {
        let mut outputs = vec![
            (
                "mbx-cache-hit".to_owned(),
                crate::yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        crate::yaml::Yaml::str("Whether the external MBX bundle was an exact hit"),
                    ),
                    (
                        "value".to_owned(),
                        crate::yaml::Yaml::str("${{ steps.mbx-bundle.outputs.cache-hit }}"),
                    ),
                ]),
            ),
            (
                "mbx-cache-key".to_owned(),
                crate::yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        crate::yaml::Yaml::str("Primary key for the external MBX bundle"),
                    ),
                    (
                        "value".to_owned(),
                        crate::yaml::Yaml::str("${{ steps.mbx-bundle-key.outputs.primary }}"),
                    ),
                ]),
            ),
        ];
        if cache_policy == PullRequestCachePolicy::SameRepositoryScoped {
            outputs.push((
                "mbx-pr-cache-allowed".to_owned(),
                crate::yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        crate::yaml::Yaml::str(
                            "Whether this run has a validated same-repository PR cache key",
                        ),
                    ),
                    (
                        "value".to_owned(),
                        crate::yaml::Yaml::str(format!(
                            "${{{{ {}}}}}",
                            crate::mbx_bundle::MBX_PR_CACHE_ALLOWED_OUTPUT
                        )),
                    ),
                ]),
            ));
        }
        outputs
    });
    let body = composite_yaml(logical, rendered, outputs)?;
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
