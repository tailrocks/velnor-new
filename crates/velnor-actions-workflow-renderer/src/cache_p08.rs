//! P08 qualified caches: built-in Mise, shared sources, Cargo-only fallback.
//!
//! Tools restore through the Mise action's built-in cache with an explicit
//! `cache_key` derived from the job's resolved tool specs (same tools share
//! one entry; job-role names never fork copies) and save through explicit
//! push-gated `Save Mise tools` steps on the elected writer per key. The
//! action's built-in save is unreachable (`install: false` disables its
//! save leg), so setups stay restore-only and never promise a save. The
//! legacy `mise-tools-*` key namespace and `ensure_tools_cache` path are
//! superseded and never emitted. Sources use one shared `actions/cache`
//! snapshot (plan writes, crates read). Cargo-only projects (no MBX
//! anywhere) use pinned `Swatinem/rust-cache` (registry-only, shared key);
//! MBX jobs never do.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::{MiseSetup, RenderError, cache_p08_detect::detector_words, setup::MISE_ACTION_NAME};

#[path = "cache_p08_shape.rs"]
mod shape;
use shape::setup_shape_ok;

#[path = "cache_p08_seed_key.rs"]
mod seed_key;
pub(crate) use seed_key::MiseToolsCacheKey;

pub use crate::cache_elect::elect_mise_cache_writers;
pub use crate::cache_elect::elect_tofu_provider_savers;

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the Cargo-only cache step.
pub const RUST_CACHE_NAME: &str = "Restore Cargo registry";
/// Owned Cargo home expression (`env:` spelling).
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";
/// Mise built-in cache key prefix.
pub const MISE_KEY_PREFIX: &str = "mise-v1";

/// Digest of sorted tool specs (16 hex chars, no `b3-` prefix).
#[must_use]
pub fn tools_digest(specs: &[String]) -> String {
    let mut sorted = specs.to_vec();
    sorted.sort();
    sorted.dedup();
    let joined = sorted.join(",");
    let digest = velnor_actions_contract::digest_b3(joined.as_bytes());
    digest.get(3..19).unwrap_or("0000000000000000").to_owned()
}

/// Qualified built-in cache key: prefix-target-mise-digest (no job id).
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for unsupported targets,
/// loose Mise versions, or malformed digests.
pub fn mise_cache_key_for_tools(
    target: &str,
    mise_version: &str,
    specs: &[String],
) -> Result<String, RenderError> {
    MiseToolsCacheKey::derive(target, mise_version, specs).map(|key| key.as_str().to_owned())
}

/// Union of `mise install`/`exec` specs across a job's shell steps.
#[must_use]
pub fn infer_job_tools(job: &Job) -> Vec<String> {
    let mut specs = BTreeSet::new();
    for step in &job.steps {
        let StepKind::Shell { run, .. } = &step.kind else {
            continue;
        };
        specs.extend(specs_in_argv(run));
    }
    specs.into_iter().collect()
}

/// Tool specs (`<tool>@<version>`) in one fixed argv.
fn specs_in_argv(run: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut take = false;
    for arg in detector_words(run) {
        if arg == "install" || arg == "exec" {
            take = true;
            continue;
        }
        if arg == "--" {
            take = false;
            continue;
        }
        if take && arg.contains('@') && is_tool_spec(&arg) {
            out.push(arg);
        }
    }
    out
}

/// Setup step with qualified built-in cache (`cache:true` + `cache_key`).
///
/// Restore-only: every run restores the tools cache, but no setup ever
/// saves through the action. The pinned `jdx/mise-action` saves only
/// inside its `install` leg, which Velnor disables (`install: false`
/// keeps project tool files, tasks, and hooks from running), so a
/// push-gated `cache_save` expression would promise a save the action
/// never performs. Push-gated saves are explicit `Save Mise tools`
/// steps on the elected writer of each key
/// ([`elect_mise_cache_writers`]); same-repo and fork PRs restore
/// read-only, since the pinned action has no PR-scoped save to promote
/// into.
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
            ("cache".to_owned(), "true".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
            ("cache_key".to_owned(), cache_key.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::MiseSetup);
    Ok(step)
}

/// True for catalog version spellings (`2026.9.18`); never `latest`.
fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}

/// True for `<tool>@<version>` specs (backend paths allowed).
fn is_tool_spec(value: &str) -> bool {
    let Some((tool, version)) = value.split_once('@') else {
        return false;
    };
    !tool.is_empty()
        && !version.is_empty()
        && !value.contains(' ')
        && !value.contains('\n')
        && tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'-' | b'_' | b'.'))
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

/// True for qualified `mise-v1-<target>-<mise>-<16hex>` keys.
pub(crate) fn is_cache_key(value: &str) -> bool {
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

/// Ensure a qualified built-in-cache setup precedes every `mise` use.
///
/// Infers the job's tool union from its fixed argv, keys the built-in
/// cache on that union (same tools share; roles never fork), and inserts
/// the setup after the configured Checkout (or upgrades a legacy `cache:false` setup in
/// place). Jobs without `mise` use (and `always=false`) stay untouched.
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
    checkout_uses: &str,
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
        let key = expected_job_key(job, setup, always, target)?.ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("setup_mise_malformed:{job_id}"))
        })?;
        upgrade_setup(job_id, job, index, setup, &key)?;
        let setup_at = crate::tool_seed::insert_before_setup(job, index, checkout_uses, &key)?;
        check_setup_before_mise(job_id, job, setup_at)?;
        return Ok(());
    }
    if always || job_uses_mise(job) {
        let Some(key) = expected_job_key(job, setup, always, target)? else {
            return Ok(());
        };
        let at = insert_at(job, checkout_uses).min(job.steps.len());
        job.steps
            .insert(at, mise_setup_step_p08(setup, key.as_str())?);
        let setup_at = crate::tool_seed::insert_before_setup(job, at, checkout_uses, &key)?;
        check_setup_before_mise(job_id, job, setup_at)?;
    } else {
        crate::tool_seed::reject_orphan_seed(job_id, job)?;
    }
    Ok(())
}

fn expected_job_key(
    job: &Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
) -> Result<Option<MiseToolsCacheKey>, RenderError> {
    let mut specs = infer_job_tools(job);
    if specs.is_empty() {
        if !always {
            return Ok(None);
        }
        specs.push("mise@bootstrap".to_owned());
    }
    MiseToolsCacheKey::derive(target, &setup.version, &specs).map(Some)
}

/// Upgrade one present setup to the qualified shape (or validate it).
fn upgrade_setup(
    job_id: &str,
    job: &mut Job,
    index: usize,
    setup: &MiseSetup,
    key: &MiseToolsCacheKey,
) -> Result<(), RenderError> {
    if job.steps[index]
        .role
        .is_some_and(|role| role != StepRole::MiseSetup)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_role_mismatch:{job_id}"
        )));
    }
    if setup_shape_ok(&job.steps[index], setup, true, Some(key)) {
        job.steps[index].role = Some(StepRole::MiseSetup);
        return Ok(());
    }
    if setup_shape_ok(&job.steps[index], setup, true, None) {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_pin_mismatch:{job_id}"
        )));
    }
    if !setup_shape_ok(&job.steps[index], setup, false, None) {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_malformed:{job_id}"
        )));
    }
    job.steps[index] = mise_setup_step_p08(setup, key.as_str())?;
    Ok(())
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
        matches!(&step.kind, StepKind::Shell { run, .. } if detector_words(run).iter().any(|word| word == "mise"))
    })
}

/// Insert after the configured Checkout, else at the front.
fn insert_at(job: &Job, checkout_uses: &str) -> usize {
    job.steps
        .iter()
        .position(|step| is_configured_checkout(step, checkout_uses))
        .map_or(0, |index| index + 1)
}

/// Reject `Swatinem/rust-cache` in MBX jobs (P08-6/7: one owner).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] when a job carries both.
pub fn check_no_rust_cache_with_mbx(job_id: &str, job: &Job) -> Result<(), RenderError> {
    let has_mbx = job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("jdx/mr-boxington-action@"))
    });
    let has_rust_cache = job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("Swatinem/rust-cache@"))
    });
    if has_mbx && has_rust_cache {
        return Err(RenderError::InvalidWorkflow(format!(
            "rust_cache_with_mbx:{job_id}"
        )));
    }
    Ok(())
}

/// Require MBX objects restore before every fetch step (P08-4).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] when fetch precedes MBX.
pub fn check_mbx_before_fetch(job_id: &str, job: &Job) -> Result<(), RenderError> {
    let at = |role| job.steps.iter().position(|step| step.role == Some(role));
    let mbx = at(StepRole::MbxCache);
    let fetch = at(StepRole::CargoSourcesFetch);
    if let (Some(mbx), Some(fetch_at)) = (mbx, fetch)
        && fetch_at < mbx
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "fetch_before_mbx:{job_id}"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "cache_p08_setup_tests.rs"]
mod setup_tests;
