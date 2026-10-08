//! Shared composite actions keep duplicated verification lanes under GitHub's workflow-size limit.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

use velnor_actions_workflow_jobs::RenderContext;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::rendered::RenderedFile;

/// One renderer-owned local composite invocation and its closed input set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedActionCall {
    /// Canonical workspace-relative action directory.
    pub uses: String,
    /// Inputs with renderer-owned value sources.
    pub inputs: Vec<SharedActionInput>,
}

/// Supported local composite inputs; values never come from arbitrary config strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedActionInput {
    /// Forward the plan's exact covered-task channel to a static task composite.
    CoveredTasks,
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
pub struct LaneShare {
    /// Same jobs, with shared lanes keeping their headers and elected saves.
    pub jobs: BTreeMap<String, Job>,
    /// Job id to local `uses` path and typed input bindings.
    pub calls: BTreeMap<String, SharedActionCall>,
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
    hosted_prefix: Vec<Step>,
    local_prefix: Vec<Step>,
    hosted_prelude: Vec<Step>,
    local_prelude: Vec<Step>,
    common: Vec<Step>,
    hosted_postlude: Vec<Step>,
    local_postlude: Vec<Step>,
}

/// Factor matching hosted/local step lists while preserving job semantics.
/// Lane headers, outputs, report uploads, and elected cache saves stay outer.
///
/// # Errors
///
/// Returns an error when a lane pair is incompatible or has an unsafe id.
pub fn share_lanes(
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
    add_provider_admission_file(jobs, ctx, &mut files)?;
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
        files.push(crate::lane_share_sections::composite_file(
            logical,
            &parts.common,
            ctx,
        )?);
        let call = SharedActionCall {
            uses,
            inputs: Vec::new(),
        };
        calls.insert(hosted_id.clone(), call.clone());
        calls.insert(local_id.clone(), call);
        checkouts.insert(hosted_id.clone(), parts.checkout.clone());
        checkouts.insert(local_id.clone(), parts.checkout);
        prefixes.insert(hosted_id.clone(), parts.hosted_prefix);
        prefixes.insert(local_id.clone(), parts.local_prefix);
        preludes.insert(hosted_id.clone(), parts.hosted_prelude);
        preludes.insert(local_id.clone(), parts.local_prelude);
        env_steps.insert(hosted_id.clone(), hosted.steps.clone());
        env_steps.insert(local_id.clone(), local.steps.clone());
        set_steps(&mut next, &hosted_id, parts.hosted_postlude.clone());
        set_steps(&mut next, &local_id, parts.local_postlude.clone());
        postludes.insert(hosted_id.clone(), parts.hosted_postlude);
        postludes.insert(local_id.clone(), parts.local_postlude);
    }
    crate::lane_share_sections::factor_provider_preludes(&mut preludes, &mut files, ctx)?;
    let mut shared = LaneShare {
        jobs: next,
        calls,
        checkouts,
        env_steps,
        prefixes,
        preludes,
        postludes,
        files,
    };
    crate::document_lanes::factor_task_report_jobs(jobs, &mut shared, ctx)?;
    validate_serialized_scopes(&shared)?;
    Ok(shared)
}

fn add_provider_admission_file(
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    files: &mut Vec<RenderedFile>,
) -> Result<(), RenderError> {
    let restores_providers = jobs.values().any(|job| {
        job.steps
            .iter()
            .any(|step| step.role == Some(StepRole::TofuProvidersRestore))
    });
    if restores_providers {
        files.push(
            velnor_actions_workflow_cache::tofu_cache::provider_admission_file(
                &ctx.generator_version,
            )?,
        );
    }
    Ok(())
}

/// Validate the expanded workflow-job and composite-action step scopes.
fn validate_serialized_scopes(shared: &LaneShare) -> Result<(), RenderError> {
    // Validate each full source job before its common body moves into a
    // composite. Provider restore/admission/use can span the hosted/local
    // prelude and the shared composite, so validating only the serialized
    // workflow steps and composite body independently would lose that order.
    for (id, steps) in &shared.env_steps {
        velnor_actions_contract_workflow::workflow::step_identity::validate_step_sequence(
            steps, id,
        )
        .map_err(RenderError::Contract)?;
    }
    for (id, job) in &shared.jobs {
        let Some(checkout) = shared.checkouts.get(id) else {
            velnor_actions_contract_workflow::workflow::step_identity::validate_step_sequence(
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
        velnor_actions_contract_workflow::workflow::step_identity::validate_step_identity_scope(
            &steps, id,
        )
        .map_err(RenderError::Contract)?;
    }
    Ok(())
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
        || !crate::lane_share_sections::same_or_admitted_check_condition(hosted, local)
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
    let (hosted_cache_prefix, hosted_steps) =
        peel_mise_cache_setup_prefix(&hosted.runs_on, hosted_steps);
    let (local_cache_prefix, local_steps) =
        peel_mise_cache_setup_prefix(&local.runs_on, local_steps);
    split_shared_steps(
        hosted_checkout,
        hosted_steps,
        local_steps,
        hosted_cache_prefix,
        local_cache_prefix,
    )
}

fn peel_mise_cache_setup_prefix<'a>(runs_on: &str, steps: &'a [Step]) -> (Vec<Step>, &'a [Step]) {
    let mut end = 0;
    for step in steps {
        let is_typed_setup =
            velnor_actions_workflow_cache::cache_p08::is_canonical_provider_cache_setup_step(step);
        let is_hosted_identity =
            velnor_actions_workflow_cache::cache_p08::is_canonical_hosted_runtime_identity_step(
                runs_on, step,
            );
        if !is_typed_setup && !is_hosted_identity {
            break;
        }
        end += 1;
    }
    (steps[..end].to_vec(), &steps[end..])
}

fn split_shared_steps(
    checkout: &Step,
    hosted_steps: &[Step],
    local_steps: &[Step],
    hosted_cache_prefix: Vec<Step>,
    local_cache_prefix: Vec<Step>,
) -> Option<SharedLaneParts> {
    let (prefix, mut hosted_prelude, mut local_prelude, hosted_tail, local_tail) =
        crate::lane_share_sections::split_mbx_steps(hosted_steps, local_steps)?;
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
    let (hosted_common, hosted_lane_specific) =
        crate::lane_share_sections::peel_lane_specific(&hosted_common);
    let (local_common, local_lane_specific) =
        crate::lane_share_sections::peel_lane_specific(&local_common);
    let mut hosted_postlude = hosted_lane_specific;
    hosted_postlude.extend(hosted_cache_postlude);
    let mut local_postlude = local_lane_specific;
    local_postlude.extend(local_cache_postlude);
    if hosted_common != local_common {
        return None;
    }
    let (hosted_prefix, local_prefix, hosted_prelude, local_prelude) =
        retain_provider_cache_prefixes(
            prefix,
            hosted_cache_prefix,
            local_cache_prefix,
            hosted_prelude,
            local_prelude,
        );
    Some(SharedLaneParts {
        checkout: checkout.clone(),
        hosted_prefix,
        local_prefix,
        hosted_prelude,
        local_prelude,
        common: hosted_common,
        hosted_postlude,
        local_postlude,
    })
}
fn retain_provider_cache_prefixes(
    common_prefix: Vec<Step>,
    hosted_cache_prefix: Vec<Step>,
    local_cache_prefix: Vec<Step>,
    hosted_prelude: Vec<Step>,
    local_prelude: Vec<Step>,
) -> (Vec<Step>, Vec<Step>, Vec<Step>, Vec<Step>) {
    if hosted_cache_prefix.is_empty() && local_cache_prefix.is_empty() {
        return (
            common_prefix.clone(),
            common_prefix,
            hosted_prelude,
            local_prelude,
        );
    }
    let mut hosted_prelude_with_common = common_prefix.clone();
    hosted_prelude_with_common.extend(hosted_prelude);
    let mut local_prelude_with_common = common_prefix;
    local_prelude_with_common.extend(local_prelude);
    (
        hosted_cache_prefix,
        local_cache_prefix,
        hosted_prelude_with_common,
        local_prelude_with_common,
    )
}

pub(crate) fn peel_checkout<'a>(
    steps: &'a [Step],
    checkout_uses: &str,
) -> Option<(&'a Step, &'a [Step])> {
    let (checkout, remaining) = steps.split_first()?;
    let is_expected_checkout = checkout.role == Some(StepRole::Checkout)
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, .. }
                if uses == checkout_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
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

fn set_steps(jobs: &mut BTreeMap<String, Job>, id: &str, steps: Vec<Step>) {
    if let Some(job) = jobs.get_mut(id) {
        job.steps = steps;
    }
}

pub(crate) fn task_coverage_id(condition: &str) -> Option<&str> {
    let id = condition
        .strip_prefix("!contains(needs.plan.outputs.covered_tasks, ',")?
        .strip_suffix(",')")?;
    safe_task_id(id).then_some(id)
}

fn safe_task_id(id: &str) -> bool {
    let parts: Vec<_> = id.split('/').collect();
    let valid_parts = parts.iter().all(|part| {
        !part.is_empty()
            && *part != "."
            && *part != ".."
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
    });
    valid_parts
        && ((parts.first() == Some(&"stack") && parts.len() >= 5)
            || (parts.first() == Some(&"internal") && parts.len() == 3))
}

#[cfg(test)]
mod tests;
