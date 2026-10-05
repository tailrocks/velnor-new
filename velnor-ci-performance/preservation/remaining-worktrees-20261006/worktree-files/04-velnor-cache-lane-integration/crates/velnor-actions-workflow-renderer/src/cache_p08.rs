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

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, StepKind};

use crate::{RenderError, cache_p08_detect::detect_commands};

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
    for (index, step) in job.steps.iter().enumerate() {
        if step.name == crate::steps::MBX_PREFLIGHT_NAME {
            if let Some(tools) = exact_generated_mbx_preflight_tools(job, index) {
                specs.extend(tools);
            } else {
                specs.insert("mise@unsupported-generated-mbx-preflight".to_owned());
            }
            continue;
        }
        if crate::mbx_bundle::is_exact_generated_export_step(job, step) {
            continue;
        }
        if let Some(tools) = crate::cache_p08_detect::report_wrapper_tool_candidates(step) {
            specs.extend(tools);
            continue;
        }
        let StepKind::Shell { run, .. } = &step.kind else {
            continue;
        };
        specs.extend(specs_in_argv(run));
    }
    specs.into_iter().collect()
}

/// Recognize only the preflight and action pair emitted by the MBX renderer.
///
/// The generated preflight uses `mise where`, shell checks, and verified
/// executable variables. Shell inference must not parse those operations as
/// repository commands. Rebuilding the adjacent pair binds the accepted pins,
/// action ref, env, and complete script to the renderer contract.
fn exact_generated_mbx_preflight_tools(job: &Job, index: usize) -> Option<[String; 2]> {
    let preflight = job.steps.get(index)?;
    let StepKind::Shell { run, env } = &preflight.kind else {
        return None;
    };
    if run.len() != 3 || run[0] != "sh" || run[1] != "-c" {
        return None;
    }
    let mbx_version = exact_preflight_version(
        run.get(2)?,
        "mise --no-config --no-env --no-hooks where 'mr-boxington@",
    )?;
    let rust_toolchain = exact_preflight_version(
        run.get(2)?,
        "mise --no-config --no-env --no-hooks where 'rust@",
    )?;
    let action = job.steps.get(index + 1)?;
    let StepKind::Action { uses, .. } = &action.kind else {
        return None;
    };
    if job
        .steps
        .iter()
        .filter(|step| crate::cache_steps::is_mbx_action(step))
        .count()
        != 1
        || job
            .steps
            .iter()
            .filter(|step| step.name == crate::steps::MBX_PREFLIGHT_NAME)
            .count()
            != 1
    {
        return None;
    }
    let scrub = crate::toolchain_env::credential_scrub();
    let mut base_env = env.clone();
    for (key, value) in scrub {
        if env.get(&key) != Some(&value) {
            return None;
        }
        base_env.remove(&key);
    }
    if !exact_mbx_preflight_env(&base_env, &rust_toolchain) {
        return None;
    }
    let [expected_preflight, expected_action] = crate::steps::mbx_steps_for_driver(
        uses,
        crate::steps::CompileDriver::Mbx,
        &mbx_version,
        &rust_toolchain,
        base_env,
    )
    .ok()??;
    let expected_pair = if job_has_finalized_mbx_transport(job) {
        finalized_mbx_pair_matches(job, index, expected_preflight, expected_action)
    } else {
        &expected_preflight == preflight && &expected_action == action
    };
    if !expected_pair {
        return None;
    }
    Some([
        format!("mr-boxington@{mbx_version}"),
        format!("rust@{rust_toolchain}"),
    ])
}

/// True after `merged_jobs` added the renderer-owned MBX bundle route.
fn job_has_finalized_mbx_transport(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            step.name.as_str(),
            crate::mbx_bundle::MBX_STORE_INIT_NAME
                | crate::mbx_bundle::MBX_BUNDLE_KEY_NAME
                | crate::mbx_bundle::MBX_BUNDLE_RESTORE_NAME
                | crate::mbx_bundle::MBX_BUNDLE_IMPORT_NAME
                | crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME
                | crate::mbx_bundle::MBX_BUNDLE_SAVE_NAME
        )
    })
}

/// Match the exact action shape after the production MBX transport transform.
fn finalized_mbx_pair_matches(
    job: &Job,
    index: usize,
    expected_preflight: velnor_actions_contract::Step,
    expected_action: velnor_actions_contract::Step,
) -> bool {
    let mut expected_job = job.clone();
    expected_job.steps[index] = expected_preflight;
    expected_job.steps[index + 1] = expected_action;
    let mut jobs = BTreeMap::from([("cache-inference".to_owned(), expected_job)]);
    if crate::mbx_bundle::append_single_bundle_saves(&mut jobs).is_err() {
        return false;
    }
    let Some(expected_job) = jobs.get("cache-inference") else {
        return false;
    };
    let Some(expected_preflight) = expected_job
        .steps
        .iter()
        .find(|step| step.name == crate::steps::MBX_PREFLIGHT_NAME)
    else {
        return false;
    };
    let Some(expected_action) = expected_job
        .steps
        .iter()
        .find(|step| crate::cache_steps::is_mbx_action(step))
    else {
        return false;
    };
    job.steps.get(index) == Some(expected_preflight)
        && job.steps.get(index + 1) == Some(expected_action)
}

/// Require the canonical private homes and exact tool isolation env.
fn exact_mbx_preflight_env(env: &std::collections::BTreeMap<String, String>, rust: &str) -> bool {
    let rustup_home = "${{ github.workspace }}/.velnor/cache/rustup";
    let expected = std::collections::BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            crate::toolchain_env::MISE_DATA_DIR_EXPR.to_owned(),
        ),
        ("MISE_RUSTUP_HOME".to_owned(), rustup_home.to_owned()),
        ("RUSTUP_HOME".to_owned(), rustup_home.to_owned()),
        ("MISE_CARGO_HOME".to_owned(), CARGO_HOME_EXPR.to_owned()),
        ("CARGO_HOME".to_owned(), CARGO_HOME_EXPR.to_owned()),
        ("RUSTUP_TOOLCHAIN".to_owned(), rust.to_owned()),
    ]);
    env == &expected
}

/// Extract one exact pin from a candidate generated preflight script.
fn exact_preflight_version(script: &str, marker: &str) -> Option<String> {
    if script.matches(marker).count() != 1 {
        return None;
    }
    let value = script.split_once(marker)?.1.split_once('\'')?.0;
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    Some(value.to_owned())
}

/// Tool specs (`<tool>@<version>`) in one fixed argv.
fn specs_in_argv(run: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let detection = detect_commands(run);
    for command in &detection.commands {
        out.extend(command.tool_candidates());
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

/// Exact numeric triple required by the pinned tool catalogue.
fn is_exact_tool_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let (Some(major), Some(minor), Some(patch)) = (parts.next(), parts.next(), parts.next()) else {
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
