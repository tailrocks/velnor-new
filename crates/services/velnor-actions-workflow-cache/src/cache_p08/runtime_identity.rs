//! Runtime identity gate for the hosted Mise cache.
//!
//! The cache is enabled only when the actual runner context and image
//! identity match a recognized hosted profile. Scale Set jobs carry no
//! authenticated image digest, so their optional Mise cache stays disabled.

use std::collections::BTreeMap;

use velnor_actions_contract_config::config::RunsOn;
use velnor_actions_contract_workflow::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract_workflow::{Job, Step, StepKind};
use velnor_actions_workflow_steps::{MiseSetup, RenderError};

use super::{
    MiseToolsCacheKey, check_setup_before_mise, infer_job_tools, insert_at, is_setup_step,
    job_uses_mise, mise_setup_step_p08, upgrade_setup,
};

const CACHE_IDENTITY_STEP_NAME: &str = "Resolve hosted Mise cache identity";
const RUNNER_ENVIRONMENT_KEY: &str = "VELNOR_CACHE_RUNNER_ENVIRONMENT";
const RUNNER_OS_KEY: &str = "VELNOR_CACHE_RUNNER_OS";
const RUNNER_ARCH_KEY: &str = "VELNOR_CACHE_RUNNER_ARCH";
const EXPECTED_IMAGE_OS_KEY: &str = "VELNOR_CACHE_EXPECTED_IMAGE_OS";
const EXPECTED_RUNNER_OS_KEY: &str = "VELNOR_CACHE_EXPECTED_RUNNER_OS";
const EXPECTED_RUNNER_ARCH_KEY: &str = "VELNOR_CACHE_EXPECTED_RUNNER_ARCH";

const CACHE_IDENTITY_SCRIPT: &str = r#"set -eu; enabled=false; suffix=disabled; if [ "$VELNOR_CACHE_RUNNER_ENVIRONMENT" = github-hosted ] && [ "$VELNOR_CACHE_RUNNER_OS" = "$VELNOR_CACHE_EXPECTED_RUNNER_OS" ] && [ "$VELNOR_CACHE_RUNNER_ARCH" = "$VELNOR_CACHE_EXPECTED_RUNNER_ARCH" ] && [ "${ImageOS:-}" = "$VELNOR_CACHE_EXPECTED_IMAGE_OS" ]; then image_version=${ImageVersion:-}; case "$image_version" in ''|*[!0-9.]*|.*|*..*|*.) ;; *) if [ "${#image_version}" -le 64 ] && [ "$image_version" != latest ]; then enabled=true; suffix="$ImageOS-$image_version"; fi ;; esac; fi; if [ -z "${GITHUB_ENV:-}" ] || [ ! -f "$GITHUB_ENV" ] || [ -L "$GITHUB_ENV" ]; then printf '%s\n' 'GITHUB_ENV must be a regular runner file' >&2; exit 1; fi; printf 'VELNOR_MISE_CACHE_ENABLED=%s\nVELNOR_MISE_CACHE_SUFFIX=%s\n' "$enabled" "$suffix" >> "$GITHUB_ENV""#;

/// Ensure the Mise setup is keyed by an authenticated runtime image identity.
///
/// The cache backend supplies repository and branch scoping. The generator
/// adds a hosted-provider discriminator and runtime image identity; untrusted
/// or unknown providers receive the pinned setup without cache access.
/// # Errors
/// Returns [`RenderError`] for malformed or misordered setup steps.
pub fn ensure_setup_p08(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup.validate()?;
    let Some(profile) = hosted_profile(&job.runs_on, target) else {
        return ensure_uncached_setup(job_id, job, setup, always, checkout_uses);
    };

    let present = setup_indices(job);
    if present.len() > 1 {
        return Err(invalid(job_id, "duplicate_setup_mise"));
    }
    if present.is_empty() && !always && !job_uses_mise(job) {
        reject_orphan_cache_steps(job_id, job)?;
        return Ok(());
    }
    let key = expected_job_key(job, setup, always, target, profile.image_os)?
        .ok_or_else(|| invalid(job_id, "setup_mise_malformed"))?;
    if let Some(&index) = present.first() {
        upgrade_setup(job_id, job, index, setup, &key)?;
    } else {
        let at = insert_at(job, checkout_uses).min(job.steps.len());
        job.steps
            .insert(at, mise_setup_step_p08(setup, key.as_str())?);
    }

    let setup_at = setup_indices(job)
        .first()
        .copied()
        .ok_or_else(|| invalid(job_id, "setup_missing"))?;
    ensure_runtime_identity_step(job_id, job, setup_at, checkout_uses, profile)?;
    let setup_at = setup_indices(job)
        .first()
        .copied()
        .ok_or_else(|| invalid(job_id, "setup_missing"))?;
    let setup_at = crate::tool_seed::insert_before_setup(job, setup_at, checkout_uses, &key)?;
    check_setup_before_mise(job_id, job, setup_at)
}

fn ensure_uncached_setup(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    reject_orphan_runtime_identity(job_id, job)?;
    let present = setup_indices(job);
    if present.len() > 1 {
        return Err(invalid(job_id, "duplicate_setup_mise"));
    }
    if present.is_empty() && !always && !job_uses_mise(job) {
        crate::tool_seed::reject_orphan_seed(job_id, job)?;
        return Ok(());
    }
    let step = velnor_actions_workflow_steps::setup::mise_setup_step(setup)?;
    if let Some(&index) = present.first() {
        let current = &job.steps[index];
        let cache_shape = super::shape::setup_shape_ok(current, setup, true, None)
            && matches!(
                &current.kind,
                StepKind::Action { with, .. }
                    if with.get("cache_key").is_some_and(|key| super::is_cache_key(key))
            );
        if !cache_shape && !super::shape::setup_shape_ok(current, setup, false, None) {
            return Err(invalid(job_id, "setup_mise_malformed"));
        }
        job.steps[index] = step;
    } else {
        let at = insert_at(job, checkout_uses).min(job.steps.len());
        job.steps.insert(at, step);
    }
    crate::tool_seed::reject_orphan_seed(job_id, job)?;
    let setup_at = setup_indices(job)
        .first()
        .copied()
        .ok_or_else(|| invalid(job_id, "setup_missing"))?;
    check_setup_before_mise(job_id, job, setup_at)
}

fn expected_job_key(
    job: &Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    image_os: &str,
) -> Result<Option<MiseToolsCacheKey>, RenderError> {
    let mut specs = infer_job_tools(job);
    if specs.is_empty() {
        if !always {
            return Ok(None);
        }
        specs.push("mise@bootstrap".to_owned());
    }
    MiseToolsCacheKey::derive(image_os, target, &setup.version, &specs).map(Some)
}

#[derive(Clone, Copy)]
struct HostedProfile {
    target: &'static str,
    image_os: &'static str,
    runner_os: &'static str,
    runner_arch: &'static str,
}

fn hosted_profile(runs_on: &str, target: &str) -> Option<HostedProfile> {
    let profile = hosted_profile_for_label(runs_on)?;
    (target == profile.target).then_some(profile)
}

fn hosted_profile_for_label(runs_on: &str) -> Option<HostedProfile> {
    let Ok(RunsOn::Hosted(label)) = RunsOn::parse(runs_on) else {
        return None;
    };
    Some(match label.as_str() {
        "ubuntu-22.04" => HostedProfile {
            target: "x86_64-unknown-linux-gnu",
            image_os: "ubuntu22",
            runner_os: "Linux",
            runner_arch: "X64",
        },
        "ubuntu-24.04" => HostedProfile {
            target: "x86_64-unknown-linux-gnu",
            image_os: "ubuntu24",
            runner_os: "Linux",
            runner_arch: "X64",
        },
        "ubuntu-26.04" => HostedProfile {
            target: "x86_64-unknown-linux-gnu",
            image_os: "ubuntu26",
            runner_os: "Linux",
            runner_arch: "X64",
        },
        "macos-15" => HostedProfile {
            target: "aarch64-apple-darwin",
            image_os: "macos15",
            runner_os: "macOS",
            runner_arch: "ARM64",
        },
        "macos-15-intel" => HostedProfile {
            target: "x86_64-apple-darwin",
            image_os: "macos15",
            runner_os: "macOS",
            runner_arch: "X64",
        },
        _ => return None,
    })
}

/// Recognize only the exact runtime identity probe generated for this hosted label.
///
/// This is a representation check for lane factoring; it does not authorize cache use
/// or establish that the runtime probe has executed.
#[must_use]
pub fn is_canonical_hosted_runtime_identity_step(runs_on: &str, step: &Step) -> bool {
    hosted_profile_for_label(runs_on).is_some_and(|profile| {
        runtime_identity_step(profile).is_ok_and(|expected| expected == *step)
    })
}

fn ensure_runtime_identity_step(
    job_id: &str,
    job: &mut Job,
    setup_at: usize,
    checkout_uses: &str,
    profile: HostedProfile,
) -> Result<(), RenderError> {
    let step = runtime_identity_step(profile)?;
    let indices: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| is_runtime_identity_candidate(candidate).then_some(index))
        .collect();
    if indices.len() > 1 {
        return Err(invalid(job_id, "duplicate_cache_runtime_identity"));
    }
    if let Some(&index) = indices.first() {
        if job.steps[index] != step {
            return Err(invalid(job_id, "cache_runtime_identity_malformed"));
        }
        if index >= setup_at {
            return Err(invalid(job_id, "cache_runtime_identity_misordered"));
        }
        let checkout = job
            .steps
            .iter()
            .position(|candidate| is_configured_checkout(candidate, checkout_uses));
        if checkout.is_some_and(|checkout| index <= checkout || checkout >= setup_at) {
            return Err(invalid(job_id, "cache_runtime_identity_misordered"));
        }
        return Ok(());
    }
    let index = job.steps[..setup_at]
        .iter()
        .position(|candidate| is_configured_checkout(candidate, checkout_uses))
        .map_or(0, |checkout| checkout + 1);
    if job.steps[..setup_at]
        .iter()
        .position(|candidate| is_configured_checkout(candidate, checkout_uses))
        .is_some_and(|checkout| checkout >= setup_at)
    {
        return Err(invalid(job_id, "cache_runtime_identity_without_checkout"));
    }
    job.steps.insert(index, step);
    Ok(())
}

fn runtime_identity_step(profile: HostedProfile) -> Result<Step, RenderError> {
    velnor_actions_workflow_steps::steps::shell_step(
        CACHE_IDENTITY_STEP_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            CACHE_IDENTITY_SCRIPT.to_owned(),
        ],
        BTreeMap::from([
            (
                RUNNER_ENVIRONMENT_KEY.to_owned(),
                "${{ runner.environment }}".to_owned(),
            ),
            (RUNNER_OS_KEY.to_owned(), "${{ runner.os }}".to_owned()),
            (RUNNER_ARCH_KEY.to_owned(), "${{ runner.arch }}".to_owned()),
            (
                EXPECTED_IMAGE_OS_KEY.to_owned(),
                profile.image_os.to_owned(),
            ),
            (
                EXPECTED_RUNNER_OS_KEY.to_owned(),
                profile.runner_os.to_owned(),
            ),
            (
                EXPECTED_RUNNER_ARCH_KEY.to_owned(),
                profile.runner_arch.to_owned(),
            ),
        ]),
    )
}

fn is_runtime_identity_candidate(step: &Step) -> bool {
    step.name == CACHE_IDENTITY_STEP_NAME
        || matches!(
            &step.kind,
            StepKind::Shell { env, .. }
                if env.contains_key(RUNNER_ENVIRONMENT_KEY)
                    || env.contains_key(RUNNER_OS_KEY)
                    || env.contains_key(RUNNER_ARCH_KEY)
        )
}

fn reject_orphan_runtime_identity(job_id: &str, job: &Job) -> Result<(), RenderError> {
    if job.steps.iter().any(is_runtime_identity_candidate) {
        Err(invalid(job_id, "cache_runtime_identity_without_cache"))
    } else {
        Ok(())
    }
}

fn reject_orphan_cache_steps(job_id: &str, job: &Job) -> Result<(), RenderError> {
    reject_orphan_runtime_identity(job_id, job)?;
    crate::tool_seed::reject_orphan_seed(job_id, job)
}

fn setup_indices(job: &Job) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| is_setup_step(step).then_some(index))
        .collect()
}

fn invalid(job_id: &str, reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("{reason}:{job_id}"))
}

#[cfg(test)]
#[path = "runtime_identity_tests.rs"]
mod tests;
