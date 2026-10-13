//! Exact Mise setup and tools-cache placement for P08.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use super::{
    MISE_KEY_PREFIX, RenderError, infer_job_tools, is_catalog_version, mise_cache_key_for_tools,
};
use crate::{
    cache_p08_detect::{command_starts_mise, detect_commands},
    setup::{MISE_ACTION_NAME, MiseSetup},
    steps::validate_uses,
};

/// Setup step with the elected explicit cache key.
///
/// The input carries the key for [`elect_mise_cache_writers`], but the
/// Mise action cache stays disabled. `ensure_setup_p08` inserts the one
/// explicit restore step before setup. The elected writer saves that
/// same complete payload after successful work.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins or cache keys.
pub fn mise_setup_step_p08(setup: &MiseSetup, cache_key: &str) -> Result<Step, RenderError> {
    setup.validate()?;
    if !is_cache_key(cache_key) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_key:{cache_key}"
        )));
    }
    let mut step = crate::steps::action_step(
        crate::setup::SETUP_MISE_NAME,
        &setup.uses,
        BTreeMap::from([
            ("version".to_owned(), setup.version.clone()),
            ("sha256".to_owned(), setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
            ("cache_key".to_owned(), cache_key.to_owned()),
        ]),
    )?;
    if let StepKind::Action { env, .. } = &mut step.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            crate::toolchain_env::MISE_DATA_DIR_EXPR.to_owned(),
        );
    }
    Ok(step)
}

/// True for qualified `mise-v1-<target>-<mise>-<16hex>` keys.
fn is_cache_key(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    value.starts_with(&format!("{MISE_KEY_PREFIX}-"))
        && !value.contains(' ')
        && !value.contains('\n')
        && !value.contains("${{")
        && !value.contains("latest")
        && parts.len() >= 4
        && parts.last().is_some_and(|digest| {
            digest.len() == 16 && digest.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

/// Ensure one exact tools restore and Mise setup precede every `mise` use.
///
/// Infer the job's tool union from fixed argv. Use that union for one
/// exact restore and one elected save key. Jobs without `mise` use (and
/// `always=false`) stay untouched.
///
/// # Errors
///
/// Returns [`RenderError`] for duplicate/misordered setups, malformed
/// pins, uninferable tools, or unsupported targets.
pub fn ensure_setup_p08(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    let present: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index)
        .collect();
    if present.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    if let Some(&index) = present.first() {
        let key = upgrade_setup(job_id, job, index, setup, always, target)?;
        ensure_tools_restore(job_id, job, &key)?;
        ensure_workspace_cache_guard(job_id, job)?;
        let setup_index = setup_index(job_id, job)?;
        check_setup_before_mise(job_id, job, setup_index)?;
        return Ok(());
    }
    if always || job_uses_mise(job) {
        let specs = infer_job_tools(job);
        let specs = if specs.is_empty() && always {
            vec!["mise@bootstrap".to_owned()]
        } else if specs.is_empty() {
            return Ok(());
        } else {
            specs
        };
        let key = mise_cache_key_for_tools(target, &setup.version, &specs)?;
        let at = insert_at(job).min(job.steps.len());
        job.steps.insert(at, mise_setup_step_p08(setup, &key)?);
        ensure_tools_restore(job_id, job, &key)?;
        ensure_workspace_cache_guard(job_id, job)?;
    }
    Ok(())
}

/// Upgrade one present setup to the qualified shape (or validate it).
fn upgrade_setup(
    job_id: &str,
    job: &mut Job,
    index: usize,
    setup: &MiseSetup,
    always: bool,
    target: &str,
) -> Result<String, RenderError> {
    if !setup_shape_ok(&job.steps[index], true) && !setup_shape_ok(&job.steps[index], false) {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_malformed:{job_id}"
        )));
    }
    let specs = infer_job_tools(job);
    let specs = if specs.is_empty() && always {
        vec!["mise@bootstrap".to_owned()]
    } else {
        specs
    };
    if specs.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_malformed:{job_id}"
        )));
    }
    let key = mise_cache_key_for_tools(target, &setup.version, &specs)?;
    job.steps[index] = mise_setup_step_p08(setup, &key)?;
    Ok(key)
}

/// Ensure one exact tools restore immediately precedes the Mise setup.
fn ensure_tools_restore(job_id: &str, job: &mut Job, key: &str) -> Result<(), RenderError> {
    let found: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == crate::cache_steps::TOOLS_RESTORE_NAME)
        .map(|(index, _)| index)
        .collect();
    if found.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_tools_restore:{job_id}"
        )));
    }
    let setup_at = setup_index(job_id, job)?;
    let restore = crate::cache_steps::tools_restore_step(key)?;
    if let Some(&restore_at) = found.first() {
        job.steps[restore_at] = restore.clone();
        if restore_at + 1 == setup_at {
            return Ok(());
        }
        job.steps.remove(restore_at);
    }
    let setup_at = setup_index(job_id, job)?;
    job.steps.insert(setup_at, restore);
    Ok(())
}

/// Guard private workspace cache paths before restore and repository scans.
fn ensure_workspace_cache_guard(job_id: &str, job: &mut Job) -> Result<(), RenderError> {
    const NAME: &str = crate::cache_steps::WORKSPACE_CACHE_GUARD_NAME;
    let found: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == NAME)
        .map(|(index, _)| index)
        .collect();
    if found.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_workspace_cache_guard:{job_id}"
        )));
    }
    let checkout_at = job.steps.iter().position(|step| step.name == "Checkout");
    let tools_restore_at = job
        .steps
        .iter()
        .position(|step| step.name == crate::cache_steps::TOOLS_RESTORE_NAME)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("missing_tools_restore:{job_id}")))?;
    let first_restore_at = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| is_workspace_cache_restore(step).then_some(index))
        .min()
        .unwrap_or(tools_restore_at);
    if checkout_at.is_some_and(|checkout_at| checkout_at >= first_restore_at) {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_restore_before_checkout:{job_id}"
        )));
    }
    let guard = crate::cache_steps::workspace_cache_guard_step(checkout_at.is_some())?;
    if let Some(&guard_at) = found.first() {
        job.steps[guard_at] = guard.clone();
        if checkout_at.is_none_or(|checkout_at| guard_at > checkout_at)
            && guard_at < first_restore_at
        {
            return Ok(());
        }
        job.steps.remove(guard_at);
    }
    let restore_at = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| is_workspace_cache_restore(step).then_some(index))
        .min()
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("missing_tools_restore:{job_id}")))?;
    job.steps.insert(restore_at, guard);
    Ok(())
}

/// True when a restore reads a workspace-local Velnor cache payload.
fn is_workspace_cache_restore(step: &Step) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    uses.starts_with("Swatinem/rust-cache@")
        || (uses.starts_with("actions/cache/restore@")
            && with
                .get("path")
                .is_some_and(|paths| paths.lines().any(|path| path.starts_with(".velnor/cache/"))))
}

/// Unique index of the Mise setup step.
fn setup_index(job_id: &str, job: &Job) -> Result<usize, RenderError> {
    let mut found = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(index, _)| index);
    let Some(index) = found.next() else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_setup_mise:{job_id}"
        )));
    };
    if found.next().is_some() {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_setup_mise:{job_id}"
        )));
    }
    Ok(index)
}

/// Reject setups after the first `mise` use.
fn check_setup_before_mise(job_id: &str, job: &Job, index: usize) -> Result<(), RenderError> {
    if let Some(first_mise) = first_mise_index(job)
        && index > first_mise
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_misordered:{job_id}"
        )));
    }
    Ok(())
}

/// True for `jdx/mise-action` steps regardless of shape.
fn is_setup_step(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with(&format!("{MISE_ACTION_NAME}@")))
}

/// True when any shell step invokes `mise`.
fn job_uses_mise(job: &Job) -> bool {
    first_mise_index(job).is_some()
}

/// Index of the first shell step invoking `mise`, when any.
fn first_mise_index(job: &Job) -> Option<usize> {
    job.steps.iter().position(|step| {
        matches!(&step.kind, StepKind::Shell { run, .. } if {
            let detection = detect_commands(run);
            detection.unsupported_mise_syntax
                || detection.commands.iter().any(|command| command_starts_mise(&command.words))
        })
    })
}

/// Insert after a leading Checkout step, else at the front.
fn insert_at(job: &Job) -> usize {
    job.steps
        .first()
        .filter(|step| step.name == "Checkout")
        .map_or(0, |_| 1)
}

/// True for a disabled built-in cache with either a key or no key.
fn setup_shape_ok(step: &Step, keyed: bool) -> bool {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return false;
    };
    if validate_uses(uses).is_err() {
        return false;
    }
    // Setup steps carry no step env; anything attached is foreign shape.
    let base = env.len() == 1
        && env
            .get("MISE_DATA_DIR")
            .is_some_and(|value| value == crate::toolchain_env::MISE_DATA_DIR_EXPR)
        && with.len() == usize::from(keyed) + 6
        && with.get("install").is_some_and(|v| v == "false")
        && with.get("env").is_some_and(|v| v == "false")
        && with.get("version").is_some_and(|v| is_catalog_version(v))
        && with
            .get("sha256")
            .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()));
    if !base {
        return false;
    }
    with.get("cache").is_some_and(|v| v == "false")
        && with.get("cache_save").is_some_and(|v| v == "false")
        && if keyed {
            with.get("cache_key").is_some_and(|v| is_cache_key(v))
        } else {
            !with.contains_key("cache_key")
        }
}
