//! P08 qualified caches: typed tool payload, shared sources, Cargo fallback.
//!
//! Tool restore and save use one ordered payload and one runner/image/tool
//! key. Setup Mise's implicit cache stays disabled; the explicit restore
//! precedes bootstrap verification and setup. Cargo sources remain a disjoint
//! source-only subset. Cargo-only projects use the pinned registry cache.

use std::collections::BTreeSet;

use velnor_actions_contract::{Job, StepKind};

use crate::{RenderError, cache_p08_detect::detector_words};

#[path = "cache_p08_setup.rs"]
mod setup;

pub use crate::cache_elect::elect_mise_cache_writers;
pub use crate::cache_elect::elect_tofu_provider_savers;
pub use setup::{ensure_setup_p08, mise_setup_step_p08};

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the Cargo-only cache step.
pub const RUST_CACHE_NAME: &str = "Restore Cargo registry";
/// Owned Cargo home expression (`env:` spelling).
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";

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

/// Complete tool payload key: runner, image, target, Mise binary pin, and selector union.
///
/// Runner-image identity remains an exact owned expression. The preceding
/// eligibility step makes missing metadata a normal, uncached cold path.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for unsupported targets, loose Mise
/// versions, or malformed tool selectors.
pub fn tools_cache_key_for_tools(
    target: &str,
    mise_version: &str,
    mise_sha256: &str,
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
    if !velnor_actions_contract::ids::is_lower_hex_len(mise_sha256, 64) {
        return Err(RenderError::BadCommand("bad_mise_sha256".to_owned()));
    }
    if specs.is_empty() {
        return Err(RenderError::BadCommand("empty_tool_specs".to_owned()));
    }
    for spec in specs {
        if !is_tool_spec(spec) {
            return Err(RenderError::BadCommand(format!("bad_tool_spec:{spec}")));
        }
    }
    let payload = crate::cache_steps::TOOLS_KEY_PREFIX;
    Ok(format!(
        "{payload}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-${{{{ env.VELNOR_CACHE_IMAGE_OS }}}}-${{{{ env.VELNOR_CACHE_IMAGE_VERSION }}}}-{target}-{mise_version}-{mise_sha256}-{}",
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

/// True for catalog version spellings (`2026.9.18`); never `latest`.
pub(crate) fn is_catalog_version(value: &str) -> bool {
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
pub(crate) fn is_tool_spec(value: &str) -> bool {
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
