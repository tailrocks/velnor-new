//! One composite action per duplicated verification lane.
//!
//! GitHub will not start a workflow file larger than 500 KB. Inlining both
//! lane step lists crosses that limit, so the steps live in one action and
//! each lane job keeps its own id, `runs-on`, and `needs`.

use std::collections::BTreeMap;

pub(super) use velnor_actions_contract::workflow::lanes::SCALE_SUFFIX;
use velnor_actions_contract::{Job, Step, StepRole};

use self::lane_share_pair::{SharedLaneParts, hosted_ids, logical_id, set_steps, split_pair};
use crate::RenderError;
use crate::lane_share_composite::composite_file;
use crate::render::RenderContext;
use crate::tree::RenderedFile;

#[path = "lane_share_pair.rs"]
mod lane_share_pair;

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

/// Factor `__hosted` / `__local` pairs whose step lists match.
///
/// # Errors
///
/// A pair whose timeout, condition, permissions, environment, or shared
/// steps differ fails closed. Report uploads, named-check execution identity,
/// and elected cache saves stay on the lane that owns them. An unsafe logical
/// id fails closed.
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
    let mut acquire_actions = crate::acquire_action::AcquireActions::default();
    if next.values().any(|job| {
        job.steps
            .iter()
            .any(|step| step.role == Some(StepRole::TofuProvidersRestore))
    }) {
        files.push(crate::tofu_cache::provider_admission_file(
            &ctx.generator_version,
        )?);
    }
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
        let Some(mut parts) = split_pair(hosted, local, &ctx.checkout_uses) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "lane_body_differs:{logical}"
            )));
        };
        let uses = format!("./.github/actions/{logical}");
        files.push(composite_for_pair(
            logical,
            &mut parts,
            ctx,
            &mut acquire_actions,
        )?);
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
    let shared = LaneShare {
        jobs: next,
        calls,
        checkouts,
        env_steps,
        prefixes,
        preludes,
        postludes,
        files,
    };
    finalize_shared_lanes(shared, ctx, acquire_actions)
}

fn finalize_shared_lanes(
    mut shared: LaneShare,
    ctx: &RenderContext,
    mut acquire_actions: crate::acquire_action::AcquireActions,
) -> Result<LaneShare, RenderError> {
    validate_serialized_scopes(&shared)?;
    for (id, job) in &mut shared.jobs {
        let replaced_acquire = !shared.postludes.contains_key(id)
            && acquire_actions.transform_steps(&mut job.steps, ctx)?;
        if replaced_acquire {
            velnor_actions_contract::workflow::step_identity::validate_step_sequence(
                &job.steps, id,
            )
            .map_err(RenderError::Contract)?;
        }
    }
    validate_serialized_scopes(&shared)?;
    acquire_actions.append_files(&mut shared.files);
    shared
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(shared)
}

fn composite_for_pair(
    logical: &str,
    parts: &mut SharedLaneParts,
    ctx: &RenderContext,
    acquire_actions: &mut crate::acquire_action::AcquireActions,
) -> Result<RenderedFile, RenderError> {
    acquire_actions.transform_steps(&mut parts.prefix, ctx)?;
    acquire_actions.transform_steps(&mut parts.hosted_prelude, ctx)?;
    acquire_actions.transform_steps(&mut parts.local_prelude, ctx)?;
    acquire_actions.transform_steps(&mut parts.hosted_postlude, ctx)?;
    acquire_actions.transform_steps(&mut parts.local_postlude, ctx)?;
    composite_file(logical, &parts.common, ctx, acquire_actions)
}

/// Validate the expanded workflow-job and composite-action step scopes.
fn validate_serialized_scopes(shared: &LaneShare) -> Result<(), RenderError> {
    // Validate each full source job before its common body moves into a
    // composite. Provider restore/admission/use can span the hosted/local
    // prelude and the shared composite, so validating only the serialized
    // workflow steps and composite body independently would lose that order.
    for (id, steps) in &shared.env_steps {
        velnor_actions_contract::workflow::step_identity::validate_step_sequence(steps, id)
            .map_err(RenderError::Contract)?;
    }
    for (id, job) in &shared.jobs {
        let Some(checkout) = shared.checkouts.get(id) else {
            velnor_actions_contract::workflow::step_identity::validate_step_sequence(
                &job.steps, id,
            )
            .map_err(RenderError::Contract)?;
            continue;
        };
        let mut steps = vec![checkout.clone()];
        if let Some(prefix) = shared.prefixes.get(id) {
            steps.extend(prefix.iter().cloned());
        }
        if let Some(prelude) = shared.preludes.get(id) {
            steps.extend(prelude.iter().cloned());
        }
        if let Some(postlude) = shared.postludes.get(id) {
            steps.extend(postlude.iter().cloned());
        }
        velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(&steps, id)
            .map_err(RenderError::Contract)?;
    }
    Ok(())
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

#[cfg(test)]
#[path = "lane_share_named_check_tests.rs"]
mod named_check_tests;
