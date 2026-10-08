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

use velnor_actions_contract_workflow::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

use crate::cache_p08_detect::detector_words;
use velnor_actions_workflow_steps::{MiseSetup, RenderError, setup::MISE_ACTION_NAME};

mod artifact;
pub use artifact::{has_artifact_matrix_markers, require_uncached_setup};

mod shape;
use shape::setup_shape_ok;

mod runtime_identity;
pub use runtime_identity::ensure_setup_p08;

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
pub const MISE_KEY_PREFIX: &str = "mise-v2-hosted";
/// Runtime value suffix appended only after the identity probe succeeds.
pub const MISE_CACHE_SUFFIX_EXPR: &str = "${{env.VELNOR_MISE_CACHE_SUFFIX}}";
/// Dynamic cache input enabled only for a recognized hosted runtime.
pub const MISE_CACHE_ENABLED_EXPR: &str =
    velnor_actions_contract_workflow::workflow::step_identity::MISE_CACHE_ENABLED_EXPR;

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

/// Qualified cache key: prefix-imageOS-target-Mise-digest-runtime suffix.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for unsupported targets,
/// loose Mise versions, or malformed digests.
pub fn mise_cache_key_for_tools(
    image_os: &str,
    target: &str,
    mise_version: &str,
    specs: &[String],
) -> Result<String, RenderError> {
    MiseToolsCacheKey::derive(image_os, target, mise_version, specs)
        .map(|key| key.as_str().to_owned())
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

/// Setup step with qualified hosted-runtime cache inputs.
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
    let mut step = velnor_actions_workflow_steps::steps::action_step(
        velnor_actions_workflow_steps::setup::SETUP_MISE_NAME,
        &setup.uses,
        BTreeMap::from([
            ("version".to_owned(), setup.version.clone()),
            ("sha256".to_owned(), setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), MISE_CACHE_ENABLED_EXPR.to_owned()),
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

/// True for qualified `mise-v2-hosted-...-<16hex>-<runtime suffix>` keys.
pub(crate) fn is_cache_key(value: &str) -> bool {
    let Some(base) = value.strip_suffix(&format!("-{MISE_CACHE_SUFFIX_EXPR}")) else {
        return false;
    };
    let parts: Vec<&str> = base.split('-').collect();
    base.starts_with(&format!("{MISE_KEY_PREFIX}-"))
        && !base.contains(' ')
        && !value.contains('\n')
        && !base.contains("latest")
        && parts.len() >= 6
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
mod tests;
