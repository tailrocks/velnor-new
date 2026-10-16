//! Native GitHub Actions step-level parallelism (`parallel:`, `background:`, `wait:`, `wait-all:`).
//!
//! Enforces the §6 invariants from `docs/proposed/parallelism-and-selection-contract.md`:
//! job-step scope only, max 10 concurrent background steps, no pre-wait output references,
//! no cache writes before background wait, and `ActionlintCapabilities` qualification gating.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{
    RenderError, document_lanes::JobStepContext, document_steps::step_to_yaml,
    render::RenderContext, steps, yaml::Yaml,
};

/// GitHub Actions hard limit on concurrent background/parallel steps in a single job.
pub const MAX_CONCURRENT_BACKGROUND_STEPS: usize = 10;

/// Synchronous step-group block (`parallel:`) that forks independent child steps and joins before the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelStepGroup {
    /// Display name of the parallel group step.
    pub name: String,
    /// Optional `if:` condition on the group step.
    pub condition: Option<String>,
    /// Child steps executed concurrently inside `parallel:`.
    pub steps: Vec<Step>,
}

/// Asynchronous background step (`background: true`) joined by a later [`WaitStep`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundStep {
    /// Explicit step identifier referenced by `wait:` and post-join output expressions.
    pub handle_id: String,
    /// Underlying step definition.
    pub step: Step,
}

/// Target of a [`WaitStep`] synchronization barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitTarget {
    /// Wait for specific background step handle IDs (`wait:`).
    Steps(Vec<String>),
    /// Wait for all currently active background steps (`wait-all: true`).
    All,
}

/// Synchronization step (`wait:` or `wait-all:`) that joins active background steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitStep {
    /// Display name of the wait step.
    pub name: String,
    /// Optional `if:` condition on the wait step.
    pub condition: Option<String>,
    /// Target background handles to join.
    pub target: WaitTarget,
}

/// Job step item supporting both standard sequential steps and §6 native step parallelism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeStepItem {
    /// Standard sequential step.
    Sequential(Step),
    /// Synchronous `parallel:` step group.
    Parallel(ParallelStepGroup),
    /// Asynchronous `background: true` step.
    Background(BackgroundStep),
    /// Synchronization `wait:` or `wait-all:` barrier.
    Wait(WaitStep),
}

fn invalid(msg: impl Into<String>) -> RenderError {
    RenderError::InvalidWorkflow(msg.into())
}

fn check_emission_gate(composite: bool, qualified: bool) -> Result<(), RenderError> {
    if composite {
        return Err(invalid("native_step_parallelism_composite_forbidden"));
    }
    if !qualified {
        return Err(invalid("native_step_parallelism_unqualified_actionlint"));
    }
    Ok(())
}

/// Validate a sequence of [`NativeStepItem`]s against all §6 contract invariants and tool qualification gates.
///
/// # Errors
///
/// Returns [`RenderError`] when composite scope is used, when `native_step_parallelism_qualified`
/// is false, when concurrency limits are exceeded, when background outputs are referenced before a
/// join wait, when cache writes occur before background steps join, or when background handles are
/// malformed or left unjoined.
pub fn validate_native_step_sequence(
    items: &[NativeStepItem],
    composite: bool,
    native_step_parallelism_qualified: bool,
) -> Result<(), RenderError> {
    if !items
        .iter()
        .any(|item| !matches!(item, NativeStepItem::Sequential(_)))
    {
        return Ok(());
    }
    check_emission_gate(composite, native_step_parallelism_qualified)?;

    let mut seen_handles = BTreeSet::new();
    let mut active_handles = BTreeSet::new();

    for item in items {
        match item {
            NativeStepItem::Sequential(step) => {
                check_no_active_output_refs(step, &active_handles)?;
                if is_cache_save_step(step) && !active_handles.is_empty() {
                    return Err(invalid("native_step_parallelism_cache_save_before_wait"));
                }
            }
            NativeStepItem::Parallel(group) => {
                validate_parallel_group_shape(group, active_handles.len())?;
                for child in &group.steps {
                    check_no_active_output_refs(child, &active_handles)?;
                }
            }
            NativeStepItem::Background(bg) => {
                validate_background_handle(&bg.handle_id)?;
                if !seen_handles.insert(bg.handle_id.clone()) {
                    return Err(invalid(format!(
                        "native_step_parallelism_duplicate_handle:{}",
                        bg.handle_id
                    )));
                }
                if is_cache_save_step(&bg.step) {
                    return Err(invalid("native_step_parallelism_cache_save_in_background"));
                }
                check_no_active_output_refs(&bg.step, &active_handles)?;
                if active_handles.len() + 1 > MAX_CONCURRENT_BACKGROUND_STEPS {
                    return Err(invalid(
                        "native_step_parallelism_concurrency_limit_exceeded",
                    ));
                }
                active_handles.insert(bg.handle_id.clone());
            }
            NativeStepItem::Wait(wait) => validate_wait_step(wait, &mut active_handles)?,
        }
    }

    if let Some(unjoined) = active_handles.iter().next() {
        return Err(invalid(format!(
            "native_step_parallelism_unjoined_background:{unjoined}"
        )));
    }
    Ok(())
}

/// Render a validated [`ParallelStepGroup`] to a YAML step mapping when qualified.
///
/// # Errors
///
/// Returns [`RenderError`] if composite scope is requested, if `native_step_parallelism_qualified`
/// is false, or if any child step fails validation.
pub fn render_parallel_group_yaml(
    job_id: &str,
    group: &ParallelStepGroup,
    ctx: &RenderContext,
    composite: bool,
    native_step_parallelism_qualified: bool,
) -> Result<Yaml, RenderError> {
    check_emission_gate(composite, native_step_parallelism_qualified)?;
    validate_parallel_group_shape(group, 0)?;
    steps::scan_for_private_subcommands(&group.name)?;
    let empty_env = BTreeMap::new();
    let step_ctx = JobStepContext {
        job_env: &empty_env,
        actions_read: false,
        runs_on: Some(&ctx.runs_on),
    };
    let mut children = Vec::with_capacity(group.steps.len());
    for child in &group.steps {
        children.push(step_to_yaml(job_id, child, ctx, &[], false, &step_ctx)?);
    }
    let mut entries = vec![("name".to_owned(), Yaml::str(group.name.clone()))];
    if let Some(condition) = &group.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    entries.push(("parallel".to_owned(), Yaml::Seq(children)));
    Ok(Yaml::Map(entries))
}

/// Render a validated [`BackgroundStep`] to a YAML step mapping when qualified.
///
/// # Errors
///
/// Returns [`RenderError`] if composite scope is requested, if `native_step_parallelism_qualified`
/// is false, or if the underlying step is invalid.
pub fn render_background_step_yaml(
    job_id: &str,
    bg: &BackgroundStep,
    ctx: &RenderContext,
    composite: bool,
    native_step_parallelism_qualified: bool,
) -> Result<Yaml, RenderError> {
    check_emission_gate(composite, native_step_parallelism_qualified)?;
    validate_background_handle(&bg.handle_id)?;
    if is_cache_save_step(&bg.step) {
        return Err(invalid("native_step_parallelism_cache_save_in_background"));
    }
    let empty_env = BTreeMap::new();
    let step_ctx = JobStepContext {
        job_env: &empty_env,
        actions_read: false,
        runs_on: Some(&ctx.runs_on),
    };
    let Yaml::Map(mut base_entries) = step_to_yaml(job_id, &bg.step, ctx, &[], false, &step_ctx)?
    else {
        return Err(invalid("native_step_parallelism_expected_step_map"));
    };
    base_entries.retain(|(k, _)| k != "id");
    let mut entries = Vec::with_capacity(base_entries.len() + 2);
    let mut inserted_id = false;
    for (k, v) in base_entries {
        let is_name = k == "name";
        entries.push((k, v));
        if is_name {
            entries.push(("id".to_owned(), Yaml::str(bg.handle_id.clone())));
            entries.push(("background".to_owned(), Yaml::Bool(true)));
            inserted_id = true;
        }
    }
    if !inserted_id {
        entries.insert(0, ("id".to_owned(), Yaml::str(bg.handle_id.clone())));
        entries.insert(1, ("background".to_owned(), Yaml::Bool(true)));
    }
    Ok(Yaml::Map(entries))
}

/// Render a validated [`WaitStep`] to a YAML step mapping when qualified.
///
/// # Errors
///
/// Returns [`RenderError`] if composite scope is requested, if `native_step_parallelism_qualified`
/// is false, or if the wait target is empty or invalid.
pub fn render_wait_step_yaml(
    wait: &WaitStep,
    composite: bool,
    native_step_parallelism_qualified: bool,
) -> Result<Yaml, RenderError> {
    check_emission_gate(composite, native_step_parallelism_qualified)?;
    if wait.name.trim().is_empty() {
        return Err(invalid("native_step_parallelism_empty_wait_name"));
    }
    steps::scan_for_private_subcommands(&wait.name)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(wait.name.clone()))];
    if let Some(condition) = &wait.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    match &wait.target {
        WaitTarget::Steps(handles) => {
            if handles.is_empty() {
                return Err(invalid("native_step_parallelism_empty_wait_steps"));
            }
            for handle in handles {
                validate_background_handle(handle)?;
            }
            if handles.len() == 1 {
                entries.push(("wait".to_owned(), Yaml::str(handles[0].clone())));
            } else {
                entries.push((
                    "wait".to_owned(),
                    Yaml::Seq(handles.iter().cloned().map(Yaml::str).collect()),
                ));
            }
        }
        WaitTarget::All => entries.push(("wait-all".to_owned(), Yaml::Bool(true))),
    }
    Ok(Yaml::Map(entries))
}

fn validate_parallel_group_shape(
    group: &ParallelStepGroup,
    active_background: usize,
) -> Result<(), RenderError> {
    if group.name.trim().is_empty() {
        return Err(invalid("native_step_parallelism_empty_group_name"));
    }
    if group.steps.len() < 2 {
        return Err(invalid(
            "native_step_parallelism_group_requires_multiple_steps",
        ));
    }
    if active_background + group.steps.len() > MAX_CONCURRENT_BACKGROUND_STEPS {
        return Err(invalid(
            "native_step_parallelism_concurrency_limit_exceeded",
        ));
    }
    if group.steps.iter().any(is_cache_save_step) {
        return Err(invalid(
            "native_step_parallelism_cache_save_in_parallel_group",
        ));
    }
    Ok(())
}

fn validate_background_handle(handle: &str) -> Result<(), RenderError> {
    if handle.is_empty()
        || !handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(invalid(format!(
            "native_step_parallelism_invalid_handle:{handle}"
        )));
    }
    Ok(())
}

fn validate_wait_step(
    wait: &WaitStep,
    active_handles: &mut BTreeSet<String>,
) -> Result<(), RenderError> {
    if wait.name.trim().is_empty() {
        return Err(invalid("native_step_parallelism_empty_wait_name"));
    }
    match &wait.target {
        WaitTarget::Steps(handles) => {
            if handles.is_empty() {
                return Err(invalid("native_step_parallelism_empty_wait_steps"));
            }
            for handle in handles {
                validate_background_handle(handle)?;
                if !active_handles.remove(handle) {
                    return Err(invalid(format!(
                        "native_step_parallelism_unknown_wait_target:{handle}"
                    )));
                }
            }
        }
        WaitTarget::All => {
            if active_handles.is_empty() {
                return Err(invalid("native_step_parallelism_empty_wait_all"));
            }
            active_handles.clear();
        }
    }
    Ok(())
}

fn is_cache_save_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave | StepRole::CargoSourcesSave)
    ) || matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses.starts_with("actions/cache/save@")
    )
}

fn check_no_active_output_refs(
    step: &Step,
    active_handles: &BTreeSet<String>,
) -> Result<(), RenderError> {
    for handle in active_handles {
        let pattern = format!("steps.{handle}.outputs");
        if step
            .condition
            .as_deref()
            .is_some_and(|cond| cond.contains(&pattern))
            || step_kind_contains(&step.kind, &pattern)
        {
            return Err(invalid(format!(
                "native_step_parallelism_output_before_wait:{handle}"
            )));
        }
    }
    Ok(())
}

fn step_kind_contains(kind: &StepKind, needle: &str) -> bool {
    match kind {
        StepKind::Action { with, env, .. } => {
            with.values().any(|v| v.contains(needle)) || env.values().any(|v| v.contains(needle))
        }
        StepKind::Shell { run, env } => {
            run.iter().any(|arg| arg.contains(needle)) || env.values().any(|v| v.contains(needle))
        }
        StepKind::Internal { env, .. } => env.values().any(|v| v.contains(needle)),
        StepKind::TaskExecution { .. } => false,
    }
}
