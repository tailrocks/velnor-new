//! Shared lane sections: prelude, postlude, and report uploads.

use std::collections::BTreeMap;

use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract_workflow::workflow::lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
};
use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_contract_workflow::{Step, StepRole};

use crate::document_steps::step_to_yaml;
use velnor_actions_workflow_jobs::RenderContext;
use velnor_actions_workflow_steps::{RenderError, steps};
use velnor_actions_workflow_tree::composite::composite_yaml;
use velnor_actions_workflow_tree::rendered::RenderedFile;
use velnor_actions_workflow_tree::yaml::Yaml;
use velnor_actions_workflow_tree::{marker, yaml::render_yaml};

/// Permit the one documented hosted-to-hosted named-check condition refinement.
pub(crate) fn same_or_admitted_check_condition(hosted: &Job, local: &Job) -> bool {
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

/// Separate lane-only upload/identity steps from the shared action body.
pub(crate) fn peel_lane_specific(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
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
    if is_postlude_step(step)
        || matches!(
            step.role,
            Some(StepRole::MatrixReportUpload | StepRole::CrateReportUpload)
        )
    {
        return true;
    }
    match &step.kind {
        StepKind::Shell { env, .. } | StepKind::Internal { env, .. } => {
            env.contains_key(NAMED_CHECK_JOB_ID_ENV)
                || env.contains_key(NAMED_CHECK_LANE_VARIANT_ENV)
        }
        StepKind::Action { .. } => false,
    }
}

/// Find the point where the lane-specific MBX cache prelude begins.
pub(crate) fn mbx_prelude_index(steps: &[Step]) -> Option<usize> {
    let preflight = steps
        .iter()
        .position(|step| step.role == Some(StepRole::MbxPreflight))?;
    (preflight < steps.len()).then_some(preflight)
}

/// Separate an MBX prelude from the remaining shared lane body.
pub(crate) fn peel_mbx_prelude(steps: &[Step]) -> Option<(Vec<Step>, &[Step])> {
    let mut end = 0;
    for step in steps {
        if is_mbx_prelude_step(step) {
            end += 1;
        } else {
            break;
        }
    }
    let prelude = &steps[..end];
    if prelude.first()?.role != Some(StepRole::MbxPreflight)
        || !prelude
            .iter()
            .any(velnor_actions_workflow_cache::cache_steps::is_mbx_action)
    {
        return None;
    }
    Some((prelude.to_vec(), &steps[end..]))
}

/// Keep the provider output owner in the job scope that consumes its outputs.
pub(crate) fn peel_provider_restore_prefix(steps: &[Step]) -> (Vec<Step>, &[Step]) {
    let Some(index) = steps
        .iter()
        .position(|step| step.role == Some(StepRole::TofuProvidersRestore))
    else {
        return (Vec::new(), steps);
    };
    let end = index + 1;
    (steps[..end].to_vec(), &steps[end..])
}

/// Separate writer and MBX export steps from the common lane action.
pub(crate) fn peel_postlude(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
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

fn is_mbx_prelude_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::MbxPreflight | StepRole::MbxCache | StepRole::MbxVersionCheck)
    )
}

fn is_postlude_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave)
    )
}

pub(crate) fn task_factor_error(id: &str, reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("task_composite_{reason}:{id}"))
}

pub(crate) fn validate_task_job_id(id: &str) -> Result<(), RenderError> {
    let valid = id.strip_prefix("rust-").is_some_and(|slug| {
        !slug.is_empty()
            && slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    valid
        .then_some(())
        .ok_or_else(|| task_factor_error(id, "unsafe_job_id"))
}

/// Replace repeated pre-restore step sequences with one shared local action.
///
/// The provider restore itself remains in each workflow job because the
/// elected save step consumes that job-scoped output ID. Only byte-identical
/// prefixes shared by multiple lane jobs are factored.
pub(crate) fn factor_provider_preludes(
    preludes: &mut BTreeMap<String, Vec<Step>>,
    files: &mut Vec<RenderedFile>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let mut groups: Vec<(Vec<Step>, Vec<String>)> = Vec::new();
    for (id, steps) in preludes.iter() {
        let Some(restore_at) = steps
            .iter()
            .position(|step| step.role == Some(StepRole::TofuProvidersRestore))
        else {
            continue;
        };
        if restore_at == 0 {
            continue;
        }
        let prefix = steps[..restore_at].to_vec();
        if let Some(index) = groups.iter().position(|(known, _)| known == &prefix) {
            groups[index].1.push(id.clone());
        } else {
            groups.push((prefix, vec![id.clone()]));
        }
    }

    for (index, (prefix, owners)) in groups.into_iter().enumerate() {
        if owners.len() < 2 {
            continue;
        }
        let logical = format!("tofu-provider-prelude-{index}");
        files.push(composite_file(&logical, &prefix, ctx)?);
        let call = Step {
            name: "Prepare ToFu provider prelude".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: format!(
                    "{}{index}",
                    velnor_actions_workflow_steps::action_ref::TOFU_PROVIDER_PRELUDE_ACTION_PREFIX
                ),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        };
        for owner in owners {
            let steps = preludes.get_mut(&owner).ok_or_else(|| {
                RenderError::InvalidWorkflow("provider_prelude_owner_missing".to_owned())
            })?;
            let restore_at = steps
                .iter()
                .position(|step| step.role == Some(StepRole::TofuProvidersRestore))
                .ok_or_else(|| {
                    RenderError::InvalidWorkflow("provider_restore_missing_after_factor".to_owned())
                })?;
            steps.splice(..restore_at, [call.clone()]);
        }
    }
    Ok(())
}

/// Render one workflow composite from typed steps.
pub(crate) fn composite_file(
    logical: &str,
    steps: &[Step],
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    velnor_actions_contract_workflow::workflow::step_identity::validate_step_identity_scope(
        steps,
        &format!("composite:{logical}"),
    )
    .map_err(RenderError::Contract)?;
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
    let quoted = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(&ctx.generator_version, &render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{logical}/action.yml"),
        bytes,
    })
}

type MbxStepSplit<'a> = (Vec<Step>, Vec<Step>, Vec<Step>, &'a [Step], &'a [Step]);

pub(crate) fn split_mbx_steps<'a>(
    hosted_steps: &'a [Step],
    local_steps: &'a [Step],
) -> Option<MbxStepSplit<'a>> {
    let hosted_action = has_mbx_action(hosted_steps);
    if hosted_action != has_mbx_action(local_steps) {
        return None;
    }
    if !hosted_action {
        return Some((
            Vec::new(),
            Vec::new(),
            Vec::new(),
            hosted_steps,
            local_steps,
        ));
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
    Some((
        hosted_prefix.to_vec(),
        hosted_prelude,
        local_prelude,
        hosted_tail,
        local_tail,
    ))
}

fn has_mbx_action(steps: &[Step]) -> bool {
    steps
        .iter()
        .any(velnor_actions_workflow_cache::cache_steps::is_mbx_action)
}

pub(crate) fn append_steps(
    id: &str,
    source: &BTreeMap<String, Vec<Step>>,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    step_context: &crate::document_lanes::JobStepContext<'_>,
    rendered: &mut Vec<Yaml>,
    label: &str,
) -> Result<(), RenderError> {
    let steps = source
        .get(id)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("shared_lane_missing_{label}:{id}")))?;
    for step in steps {
        rendered.push(step_to_yaml(
            id,
            step,
            ctx,
            needs_envs,
            false,
            step_context.job_env,
            step_context.actions_read,
        )?);
    }
    Ok(())
}

pub(crate) fn valid_shared_checkout(checkout: &Step, expected_uses: &str) -> bool {
    checkout.role == Some(StepRole::Checkout)
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        )
}
