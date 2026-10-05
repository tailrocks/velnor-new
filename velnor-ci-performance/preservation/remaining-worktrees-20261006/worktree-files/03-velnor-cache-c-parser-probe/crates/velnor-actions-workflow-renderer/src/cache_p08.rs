//! P08 qualified caches: exact tool payload, shared sources, Cargo-only fallback.
//!
//! Tools restore through one explicit `actions/cache` step whose exact key
//! derives from the job's resolved tool specs (same tools share one entry;
//! job-role names never fork copies). Its payload includes Mise installs,
//! Rustup toolchains, and Cargo proxy executables. A matching push-gated
//! `Save Mise tools` step runs on the elected writer per key. The Mise
//! action's cache is disabled, so it cannot duplicate the restore. The
//! legacy `mise-tools-*` key namespace and `ensure_tools_cache` path are
//! superseded and never emitted. Sources use one shared `actions/cache`
//! snapshot (plan writes, crates read). Cargo-only projects (no MBX
//! anywhere) use pinned `Swatinem/rust-cache` (registry-only, shared key);
//! MBX jobs never do.

use std::collections::BTreeSet;

use velnor_actions_contract::{Job, StepKind};

use crate::{
    RenderError,
    cache_p08_detect::{detect_commands, mise_tool_candidates},
};

#[path = "cache_p08_setup.rs"]
mod setup_p08;
pub use setup_p08::{ensure_setup_p08, mise_setup_step_p08};

pub use crate::cache_elect::elect_mise_cache_writers;
pub use crate::cache_elect::elect_tofu_provider_savers;

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the Cargo-only cache step.
pub const RUST_CACHE_NAME: &str = "Restore Cargo registry";
/// Owned Cargo home expression (`env:` spelling).
pub const CARGO_HOME_EXPR: &str = "${{ github.workspace }}/.velnor/cache/cargo";
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
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_target:{target}"
        )));
    }
    if !is_catalog_version(mise_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mise_version:{mise_version}"
        )));
    }
    if specs.is_empty() {
        return Err(RenderError::BadCommand("empty_tool_specs".to_owned()));
    }
    for spec in specs {
        if !is_tool_spec(spec) {
            return Err(RenderError::BadCommand(format!("bad_tool_spec:{spec}")));
        }
    }
    Ok(format!(
        "{MISE_KEY_PREFIX}-{target}-{mise_version}-{}",
        tools_digest(specs)
    ))
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
    let detection = detect_commands(run);
    for command in &detection.commands {
        out.extend(mise_tool_candidates(&command.words));
    }
    if detection.unsupported_mise_syntax {
        // Invalid by construction. `mise_cache_key_for_tools` rejects this
        // marker, so unknown shell syntax cannot produce an under-keyed
        // tools payload.
        out.push("mise@unsupported-shell-syntax".to_owned());
    }
    out
}

/// True for catalog version spellings (`2026.9.18`); never `latest`.
pub(super) fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}

/// True for pinned `<tool>@<version>` specs (backend paths allowed).
fn is_tool_spec(value: &str) -> bool {
    if value == "mise@bootstrap" {
        return true;
    }
    let Some((tool, version)) = value.split_once('@') else {
        return false;
    };
    !tool.is_empty()
        && !value.contains(' ')
        && !value.contains('\n')
        && tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'-' | b'_' | b'.'))
        && is_exact_tool_version(version)
}

<<<<<<< HEAD
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
/// the setup after Checkout (or upgrades a legacy `cache:false` setup in
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
        upgrade_setup(job_id, job, index, setup, target)?;
        let setup_at = crate::tool_seed::insert_before_setup(job, index)?;
        check_setup_before_mise(job_id, job, setup_at)?;
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
        let setup_at = crate::tool_seed::insert_before_setup(job, at)?;
        check_setup_before_mise(job_id, job, setup_at)?;
    }
    Ok(())
}

/// Upgrade one present setup to the qualified shape (or validate it).
fn upgrade_setup(
    job_id: &str,
    job: &mut Job,
    index: usize,
    setup: &MiseSetup,
    target: &str,
) -> Result<(), RenderError> {
    if setup_shape_ok(&job.steps[index], true) {
        return Ok(());
    }
    if !setup_shape_ok(&job.steps[index], false) {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_malformed:{job_id}"
        )));
    }
    let specs = infer_job_tools(job);
    if specs.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "setup_mise_malformed:{job_id}"
        )));
    }
    let key = mise_cache_key_for_tools(target, &setup.version, &specs)?;
    job.steps[index] = mise_setup_step_p08(setup, &key)?;
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

/// Insert after a leading Checkout step, else at the front.
fn insert_at(job: &Job) -> usize {
    job.steps
        .first()
        .filter(|step| step.name == "Checkout")
        .map_or(0, |_| 1)
}

/// True for qualified (`cache:true`+key) or legacy (`cache:false`) shapes.
fn setup_shape_ok(step: &Step, qualified: bool) -> bool {
    let StepKind::Action { uses, with, env } = &step.kind else {
=======
/// Exact numeric triple required by the pinned tool catalogue.
fn is_exact_tool_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let (Some(major), Some(minor), Some(patch)) = (parts.next(), parts.next(), parts.next()) else {
>>>>>>> c9a4f7c4e (fix(renderer): bound Mise cache command detection)
        return false;
    };
    parts.next().is_none()
        && [major, minor, patch]
            .into_iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
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
    let at = |name: &str| job.steps.iter().position(|s| s.name == name);
    let fetch = job
        .steps
        .iter()
        .position(|s| s.name.starts_with("Fetch Cargo sources"));
    if let (Some(mbx), Some(fetch_at)) = (at("Restore MBX objects"), fetch)
        && fetch_at < mbx
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "fetch_before_mbx:{job_id}"
        )));
    }
    Ok(())
}
