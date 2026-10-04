//! Qualified isolated tools and source transports.
//!
//! Compiled owner helpers acquire and verify tools in fixed isolated domains.
//! Pure producer jobs export immutable snapshots; workload jobs restore only.
//! Cargo sources use a separate owned payload.

#[path = "cache_tools_payload.rs"]
pub(crate) mod payload;
#[path = "cache_tool_phases.rs"]
pub(crate) mod phases;
use payload::ensure_tool_payload;
pub(crate) use payload::preflight::{is_preflight_argv, validate_preflight_env};
#[path = "cache_tool_inventory.rs"]
mod inventory;
#[path = "cache_tool_specs.rs"]
mod specs;
pub use inventory::{infer_job_selectors, infer_job_tools};
#[path = "cache_source_roles.rs"]
pub(crate) mod source_roles;
#[path = "cache_tool_roles.rs"]
pub(crate) mod tool_roles;
use specs::is_tool_spec;
pub use tool_roles::{producer_outputs, producer_steps, report_record};

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{MiseSetup, RenderError, cache_p08_detect::detector_words};

pub use crate::cache_elect::validate_tool_consumers;

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the Cargo-only cache step.
pub const RUST_CACHE_NAME: &str = "Restore Cargo registry";
/// Owned Cargo home expression (`env:` spelling).
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";
/// Explicit tool transport key prefix.
pub const MISE_KEY_PREFIX: &str = "mise-v3";

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

/// Qualified tool key: prefix-target-mise-digest (no job identifier).
///
/// # Errors
/// Returns [`RenderError::BadCommand`] for unsupported targets,
/// loose Mise versions, or malformed digests.
pub fn mise_cache_key_for_tools(
    target: &str,
    mise_version: &str,
    specs: &[String],
) -> Result<String, RenderError> {
    key_for_specs(target, mise_version, specs, &[])
}

pub(crate) fn mise_cache_key_for_job(
    target: &str,
    mise_version: &str,
    job: &Job,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<String, RenderError> {
    validate_job_tool_records(job, records)?;
    let footprint: Vec<_> = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::SourceBoundHelper { invocation, .. } => {
                Some(invocation.installed_selectors())
            }
            _ => None,
        })
        .flatten()
        .cloned()
        .collect();
    key_for_specs(target, mise_version, &infer_job_tools(job), &footprint)
}

pub(crate) fn validate_job_tool_records(
    job: &Job,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(), RenderError> {
    if job.steps.iter().any(|step| match &step.kind {
        StepKind::SourceBoundHelper { invocation, env } => !records
            .iter()
            .any(|record| record.invocation() == invocation && record.environment() == env),
        _ => false,
    }) {
        return Err(RenderError::InvalidWorkflow(
            "tool_helper_not_registered".to_owned(),
        ));
    }
    Ok(())
}

fn key_for_specs(
    target: &str,
    mise_version: &str,
    specs: &[String],
    footprint: &[String],
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
        if !is_tool_spec(spec) && !footprint.contains(spec) {
            return Err(RenderError::BadCommand(format!("bad_tool_spec:{spec}")));
        }
    }
    Ok(format!(
        "{MISE_KEY_PREFIX}-{target}-{mise_version}-{}",
        tools_digest(specs)
    ))
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

/// Restore exact qualified tools before the compiled bootstrap and tool use.
/// Consumers never export executable state after repository computation.
/// # Errors
/// Rejects unbound bootstrap records, altered transports and misordered tools.
pub fn ensure_setup_p08(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(), RenderError> {
    setup.validate()?;
    validate_job_tool_records(job, records)?;
    if tool_roles::validate_tool_producer(job, setup, records)? {
        return Err(RenderError::InvalidWorkflow(
            "tool_producer_requires_qualified_admission".to_owned(),
        ));
    }
    if source_roles::validate_source_producer(job, setup, records)? {
        return Ok(());
    }
    if crate::early_plan::has_early_plan(job) {
        return phases::ensure(job_id, job, setup, target, records);
    }
    let specs = infer_job_tools(job);
    if skip_ordinary_tools(job, specs.is_empty(), always)? {
        return Ok(());
    }
    let identity = if specs.is_empty() {
        mise_cache_key_for_tools(target, &setup.version, &["mise@bootstrap".to_owned()])?
    } else {
        mise_cache_key_for_job(target, &setup.version, job, records)?
    };
    let key = format!("{identity}-${{{{env.VELNOR_CACHE_IMAGE}}}}");
    let bootstrap = crate::setup::mise_setup_step(
        setup,
        velnor_actions_contract::ToolCacheDomain::Full,
        &job.runs_on,
    )?;
    let present: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_setup_step(step))
        .map(|(at, _)| at)
        .collect();
    let at = match present.as_slice() {
        [] => {
            let at = job
                .steps
                .iter()
                .position(|step| step.name == crate::steps::ACQUIRE_NAME)
                .map_or_else(
                    || {
                        usize::from(
                            job.steps
                                .first()
                                .is_some_and(|step| step.name == "Checkout"),
                        )
                    },
                    |at| at + 1,
                );
            job.steps.insert(at, bootstrap);
            at
        }
        [at] if job.steps[*at] == bootstrap => *at,
        _ => {
            return Err(RenderError::InvalidWorkflow(format!(
                "mise_bootstrap_changed_or_duplicate:{job_id}"
            )));
        }
    };
    if first_mise_index(job).is_some_and(|first| first < at) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mise_bootstrap_after_tools:{job_id}"
        )));
    }
    ensure_tool_payload(job, at, &key)
}

fn skip_ordinary_tools(job: &Job, empty: bool, always: bool) -> Result<bool, RenderError> {
    if job.steps.iter().any(|step| {
        crate::cache_tool_paths::owned_transport(step)
            && !crate::cache_tool_paths::transport_in_domain(
                step,
                velnor_actions_contract::ToolCacheDomain::Full,
            )
    }) {
        return Err(RenderError::InvalidWorkflow(
            "tool_consumer_foreign_domain".to_owned(),
        ));
    }
    if empty && !always && !job.steps.iter().any(is_setup_step) && first_mise_index(job).is_none() {
        if job
            .steps
            .iter()
            .any(crate::cache_tool_paths::owned_transport)
        {
            return Err(RenderError::InvalidWorkflow(
                "tool_consumer_unbound_restore".to_owned(),
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn is_setup_step(step: &Step) -> bool {
    matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
        if invocation.descriptor().operation() == velnor_actions_contract::SourceBoundOperation::MiseBootstrap)
        || matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("jdx/mise-action@"))
}

fn first_mise_index(job: &Job) -> Option<usize> {
    job.steps.iter().position(|step| match &step.kind {
        StepKind::Shell { run, .. } => detector_words(run)
            .iter()
            .any(|word| crate::cache_tool_paths::is_mise_word(word)),
        StepKind::SourceBoundHelper { invocation, env } => {
            invocation.descriptor().operation()
                != velnor_actions_contract::SourceBoundOperation::MiseBootstrap
                && (!invocation.installed_selectors().is_empty()
                    || env.contains_key("VELNOR_TOOL_CACHE_IDENTITY")
                    || env.contains_key("VELNOR_RUSTUP_IDENTITY")
                    || env.contains_key("VELNOR_QUALIFIED_TOOL_IDENTITY"))
        }
        _ => false,
    })
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

#[cfg(test)]
#[path = "cache_p08_key_tests.rs"]
mod key_tests;
