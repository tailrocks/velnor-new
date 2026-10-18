//! Insert the typed V2 tools-cache prelude into rendered jobs.

use velnor_actions_contract::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract::{Job, RunsOn, Step, StepKind, StepRole};

use crate::{MiseSetup, RenderError, cache_p08, cache_p08::ToolsCacheInputs};

/// Add runtime identity, read-only restore, and cache-disabled Mise setup.
/// # Errors
pub(crate) fn ensure_tools_cache_v2(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    options: cache_p08::ToolsCacheRenderOptions<'_>,
) -> Result<(), RenderError> {
    setup.validate()?;
    let setup_indices = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if setup_indices.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    let needed = always || job.steps.iter().any(step_uses_mise);
    if !needed && setup_indices.is_empty() {
        reject_v2_steps(job_id, job)?;
        return Ok(());
    }
    let expected_setup = cache_p08::setup_step(setup)?;
    let setup_index = if let Some(index) = setup_indices.first().copied() {
        if !cache_p08::same_step_semantics(&job.steps[index], &expected_setup) {
            return Err(RenderError::InvalidWorkflow(format!(
                "setup_mise_malformed:{job_id}"
            )));
        }
        move_setup_after_helper(job, index, options.checkout_uses)
    } else {
        let at = insert_at(job, options.checkout_uses).min(job.steps.len());
        job.steps.insert(at, expected_setup);
        move_setup_after_helper(job, at, options.checkout_uses)
    };
    if let Some(first_mise) = first_mise_index(job)
        && setup_index > first_mise
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_misordered:{job_id}"
        )));
    }
    reject_v2_steps(job_id, job)?;
    if !checkout_precedes_setup(job, setup_index, options.checkout_uses) {
        return Ok(());
    }
    insert_tools_prelude(
        job_id,
        job,
        setup,
        options.target,
        options.scale_set_image_source_sha,
        setup_index,
        options.require_verified_helper,
    )
}

pub(crate) fn ensure_mise_setup_only(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    let setup_indices = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if setup_indices.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    let needed = always || job.steps.iter().any(step_uses_mise);
    if !needed && setup_indices.is_empty() {
        reject_v2_steps(job_id, job)?;
        return Ok(());
    }
    let expected_setup = cache_p08::setup_step(setup)?;
    let setup_index = if let Some(index) = setup_indices.first().copied() {
        if !cache_p08::same_step_semantics(&job.steps[index], &expected_setup) {
            return Err(RenderError::InvalidWorkflow(format!(
                "setup_mise_malformed:{job_id}"
            )));
        }
        index
    } else {
        let at = insert_at(job, checkout_uses).min(job.steps.len());
        job.steps.insert(at, expected_setup);
        at
    };
    if let Some(first_mise) = first_mise_index(job)
        && setup_index > first_mise
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_misordered:{job_id}"
        )));
    }
    reject_v2_steps(job_id, job)
}

fn insert_tools_prelude(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    target: &str,
    scale_set_image_source_sha: Option<&str>,
    setup_index: usize,
    require_verified_helper: bool,
) -> Result<(), RenderError> {
    let specs = cache_p08::infer_job_tools(job);
    if specs.is_empty() {
        return Ok(());
    }
    if !job.steps.iter().any(is_helper_stage) {
        if require_verified_helper {
            return Err(RenderError::InvalidWorkflow(format!(
                "tools_cache_helper_missing:{job_id}"
            )));
        }
        return Ok(());
    }
    let rust_version = specs.iter().find_map(|spec| spec.strip_prefix("rust@"));
    let components =
        if let Some(version) = rust_version {
            job.steps
                .iter()
                .find(|step| {
                    step.role == Some(velnor_actions_contract::StepRole::PrepareRustComponents)
                })
                .map(|step| cache_p08::rust_components(step, version, target))
                .transpose()?
                .unwrap_or_default()
        } else {
            if job.steps.iter().any(|step| {
                step.role == Some(velnor_actions_contract::StepRole::PrepareRustComponents)
            }) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "rust_components_without_toolchain:{job_id}"
                )));
            }
            Vec::new()
        };
    let payload = cache_p08::ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: &job.runs_on,
        target,
        mise_setup: setup,
        tool_specs: &specs,
        rustup_toolchain: rust_version,
        rustup_components: &components,
        scale_set_image_source_sha: RunsOn::parse(&job.runs_on)
            .is_ok_and(|runs_on| runs_on.is_scale_set())
            .then_some(scale_set_image_source_sha)
            .flatten(),
    })?;
    if !payload.runtime_identity_supported() {
        return Ok(());
    }
    let needs_plan = job
        .needs
        .iter()
        .any(|dependency| dependency == crate::render::PLAN_JOB_ID);
    let mut prelude = payload.runtime_prelude_step()?;
    prelude.condition = Some(cache_p08::tools_cache_identity_condition(needs_plan).to_owned());
    let mut restore = payload.restore_step()?;
    restore.condition = Some(cache_p08::tools_cache_restore_condition(needs_plan).to_owned());
    let helper_at = job.steps.iter().rposition(is_helper_stage);
    let prelude_at = helper_at.map_or(setup_index, |index| index.saturating_add(1));
    if first_mise_index(job).is_some_and(|index| prelude_at > index) {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_helper_after_tool_use:{job_id}"
        )));
    }
    job.steps.splice(prelude_at..prelude_at, [prelude, restore]);
    Ok(())
}

fn checkout_precedes_setup(job: &Job, setup_index: usize, expected_uses: &str) -> bool {
    job.steps
        .iter()
        .position(|step| is_typed_checkout(step, expected_uses))
        .is_some_and(|checkout_index| checkout_index < setup_index)
}

fn is_typed_checkout(step: &Step, expected_uses: &str) -> bool {
    is_configured_checkout(step, expected_uses)
}

fn reject_v2_steps(job_id: &str, job: &Job) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| has_v2_cache_authority(step, job))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "renderer_owned_tools_cache_step:{job_id}"
        )));
    }
    Ok(())
}

fn has_v2_cache_authority(step: &Step, job: &Job) -> bool {
    if matches!(
        step.role,
        Some(
            velnor_actions_contract::StepRole::ToolSeed
                | velnor_actions_contract::StepRole::ToolsCacheIdentity
                | velnor_actions_contract::StepRole::ToolsCacheRestore
                | velnor_actions_contract::StepRole::ToolsCacheSave
        )
    ) {
        return true;
    }
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    if uses == velnor_actions_contract::workflow::step_identity::TOOL_SEED_USES {
        return true;
    }
    if cache_p08::runtime_identity_action_uses(&job.runs_on) == Some(uses.as_str())
        || cache_p08::runtime_prelude_action_uses(&job.runs_on) == Some(uses.as_str())
    {
        return true;
    }
    if uses == crate::cache_steps::TOOLS_RESTORE_USES {
        return true;
    }
    let tools_paths = crate::cache_steps::TOOLS_CACHE_PATHS
        .map(str::to_owned)
        .join("\n");
    uses == crate::cache_steps::TOOLS_RESTORE_ACTION_USES
        && with.get("path").map(String::as_str) == Some(tools_paths.as_str())
}

fn is_helper_stage(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::AcquireVelnor | StepRole::PreseedStage)
    )
}

fn step_uses_mise(step: &Step) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    crate::cache_p08_detect::detector_words(run)
        .iter()
        .any(|word| word == "mise")
}

fn first_mise_index(job: &Job) -> Option<usize> {
    job.steps.iter().position(step_uses_mise)
}

fn is_setup_step(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. }
        if uses.starts_with(&format!("{}@", crate::setup::MISE_ACTION_NAME)))
}

fn insert_at(job: &Job, checkout_uses: &str) -> usize {
    job.steps
        .iter()
        .position(|step| is_typed_checkout(step, checkout_uses))
        .map_or(0, |index| index + 1)
}

fn move_setup_after_helper(job: &mut Job, setup_index: usize, checkout_uses: &str) -> usize {
    let Some(checkout_index) = job
        .steps
        .iter()
        .position(|step| is_typed_checkout(step, checkout_uses))
    else {
        return setup_index;
    };
    let after_checkout = checkout_index + 1;
    let helper_index = job
        .steps
        .iter()
        .enumerate()
        .rev()
        .find(|(index, step)| *index >= after_checkout && is_helper_stage(step))
        .map(|(index, _)| index);
    let Some(helper_index) = helper_index.filter(|index| setup_index < *index) else {
        return setup_index;
    };
    let setup = job.steps.remove(setup_index);
    job.steps.insert(helper_index, setup);
    helper_index
}

pub(crate) fn move_acquire_after_checkout(job: &mut Job, checkout_uses: &str) {
    let Some(acquire_at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::AcquireVelnor))
    else {
        return;
    };
    let Some(checkout_at) = job
        .steps
        .iter()
        .position(|step| is_typed_checkout(step, checkout_uses))
    else {
        return;
    };
    if acquire_at == checkout_at + 1 {
        return;
    }
    let acquire = job.steps.remove(acquire_at);
    let checkout_at = if acquire_at < checkout_at {
        checkout_at - 1
    } else {
        checkout_at
    };
    job.steps.insert(checkout_at + 1, acquire);
}

#[path = "cache_p08_setup_components.rs"]
mod components;
pub(crate) use components::rust_components;
