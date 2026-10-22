//! Runner identity and version validation for native release signing.

use std::{env, path::PathBuf, time::Duration};

use velnor_actions_contract::{
    PackageReleasePlan,
    config::RustBinaryReleaseCompanion,
    package_release::{PackageReleaseChannel, PackageReleaseEvent},
    preview_identity::PreviewIdentity,
};

use super::{
    super::PackageReleaseSigningIdentity,
    filesystem::{is_full_sha, positive_u32, positive_u64, query_repository_id, required_env},
};
use crate::{OrchestratorError, internal::internal};

#[derive(Clone, Copy)]
struct PreviewSigningInput<'a> {
    repository_id: u64,
    source_ref: &'a str,
    event_sha: &'a str,
    github_workflow_sha: &'a str,
    run_id: u64,
    run_attempt: u32,
    ci_run_id: u64,
    ci_run_attempt: u32,
    commit_count: Option<u64>,
}

pub(in crate::package_release_signer) fn signing_identity_from_env(
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let started = std::time::Instant::now();
    let repository = required_env("GITHUB_REPOSITORY")?;
    let repository_id = query_repository_id(&repository)?;
    signing_identity_for_repository(plan, repository_id, started)
}

pub(in crate::package_release_signer) fn signing_identity_for_repository(
    plan: &PackageReleasePlan,
    repository_id: u64,
    started: std::time::Instant,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let identity = identity_for_repository(plan, repository_id)?;
    if plan.channel == PackageReleaseChannel::Stable {
        let deadline = velnor_actions_mise::CheckDeadline::from_start_with_cleanup_reserve(
            started,
            Duration::from_secs(
                u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
            ),
        )
        .map_err(|_| internal("package_signer_deadline"))?;
        let admitted = crate::stable_tag_origin::admitted_production_context(plan, deadline)?;
        require_stable_context(plan, &identity, &admitted)?;
    }
    Ok(identity)
}

// Published reads authenticate the exact current Tap caller and remote stable
// origin downstream; they never acquire signing authority from this identity.
pub(in crate::package_release_signer) fn published_identity_from_env(
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let repository_id = query_repository_id(&required_env("GITHUB_REPOSITORY")?)?;
    identity_for_repository(plan, repository_id)
}

fn identity_for_repository(
    plan: &PackageReleasePlan,
    repository_id: u64,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let repository = required_env("GITHUB_REPOSITORY")?;
    let event_sha = required_env("GITHUB_SHA")?;
    let source_ref = required_env("GITHUB_REF")?;
    let workflow_ref = required_env("GITHUB_WORKFLOW_REF")?;
    let github_workflow_sha = required_env("GITHUB_WORKFLOW_SHA")?;
    let event = signing_event(plan, &required_env("GITHUB_EVENT_NAME")?)?;
    let run_id = positive_u64("GITHUB_RUN_ID")?;
    let run_attempt = positive_u32("GITHUB_RUN_ATTEMPT")?;
    let ci_run_id = positive_u64("VELNOR_RELEASE_CI_RUN_ID")?;
    let ci_run_attempt = positive_u32("VELNOR_RELEASE_CI_RUN_ATTEMPT")?;
    let commit_count = match plan.channel {
        PackageReleaseChannel::Preview => Some(positive_u64("VELNOR_RELEASE_COMMIT_COUNT")?),
        PackageReleaseChannel::Stable => None,
    };
    let (source_sha, workflow_sha) = preview_signing_authorities(
        plan,
        PreviewSigningInput {
            repository_id,
            source_ref: &source_ref,
            event_sha: &event_sha,
            github_workflow_sha: &github_workflow_sha,
            run_id,
            run_attempt,
            ci_run_id,
            ci_run_attempt,
            commit_count,
        },
    )?;
    let identity = PackageReleaseSigningIdentity {
        repository,
        repository_id,
        source_sha,
        source_ref,
        default_branch: plan.default_branch.clone(),
        event,
        workflow_path: plan.channel.workflow_path().to_owned(),
        workflow_ref,
        workflow_sha,
        run_id,
        run_attempt,
        ci_run_id,
        ci_run_attempt,
        commit_count,
    };
    validate_identity(plan, &identity)?;
    Ok(identity)
}

fn preview_signing_authorities(
    plan: &PackageReleasePlan,
    input: PreviewSigningInput<'_>,
) -> Result<(String, String), OrchestratorError> {
    let PreviewSigningInput {
        repository_id,
        source_ref,
        event_sha,
        github_workflow_sha,
        run_id,
        run_attempt,
        ci_run_id,
        ci_run_attempt,
        commit_count,
    } = input;
    if plan.channel == PackageReleaseChannel::Preview
        && matches!(
            env::var("VELNOR_INTERNAL_OP").ok().as_deref(),
            Some("sign-package-release-v1" | "publish-package-release-v1")
        )
    {
        let source_sha = required_env("VELNOR_RELEASE_SOURCE_SHA")?;
        let workflow_authority_sha = required_env("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?;
        let temp = PathBuf::from(required_env("RUNNER_TEMP")?).join("velnor");
        if !temp.is_absolute() {
            return Err(internal("package_signer_runner_temp_invalid"));
        }
        let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(300))
            .map_err(|_| internal("package_signer_deadline"))?;
        let admitted = crate::package_release_publish::release_policy_admission::admitted_context(
            &temp, deadline,
        )?;
        let expected_version = velnor_actions_contract::preview_identity::PreviewIdentity::derive(
            &plan.base_version,
            &source_sha,
            commit_count.ok_or_else(|| internal("package_signer_commit_count_missing"))?,
        )
        .map_err(|_| internal("package_signer_preview_version_invalid"))?;
        if !is_full_sha(&source_sha)
            || !is_full_sha(event_sha)
            || !is_full_sha(github_workflow_sha)
            || event_sha != workflow_authority_sha
            || github_workflow_sha != workflow_authority_sha
            || admitted.repository_id != repository_id
            || admitted.head_repository_id != repository_id
            || admitted.source_sha != source_sha
            || admitted.workflow_authority_sha != workflow_authority_sha
            || admitted.source_ref != source_ref
            || admitted.run_id != run_id
            || admitted.run_attempt != run_attempt
            || admitted.ci_run_id != ci_run_id
            || admitted.ci_run_attempt != ci_run_attempt
            || Some(admitted.commit_count) != commit_count
            || admitted.version != expected_version.version
        {
            return Err(internal("package_signer_authority_mismatch"));
        }
        return Ok((source_sha, workflow_authority_sha));
    }
    Ok((event_sha.to_owned(), github_workflow_sha.to_owned()))
}

pub(in crate::package_release_signer) fn require_stable_context(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    admitted: &velnor_actions_contract::preview_production::ReleaseProductionContext,
) -> Result<(), OrchestratorError> {
    if plan.channel != PackageReleaseChannel::Stable
        || admitted.repository_id != identity.repository_id
        || admitted.head_repository_id != identity.repository_id
        || admitted.source_sha != identity.source_sha
        || admitted.source_ref != identity.source_ref
        || admitted.workflow_authority_sha != identity.workflow_sha
        || admitted.run_id != identity.run_id
        || admitted.run_attempt != identity.run_attempt
        || admitted.ci_run_id != identity.ci_run_id
        || admitted.ci_run_attempt != identity.ci_run_attempt
        || admitted.commit_count != 0
        || admitted.version != plan.base_version
    {
        return Err(internal("package_signer_stable_origin_context_mismatch"));
    }
    Ok(())
}

pub(in crate::package_release_signer) fn signing_event(
    plan: &PackageReleasePlan,
    actual: &str,
) -> Result<PackageReleaseEvent, OrchestratorError> {
    let expected = plan.channel.event();
    if actual != expected.as_str() {
        return Err(internal("package_signer_event_invalid"));
    }
    Ok(expected)
}

pub(in crate::package_release_signer) fn validate_identity(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
) -> Result<(), OrchestratorError> {
    let workflow_ref = format!(
        "{}/{}@{}",
        identity.repository, identity.workflow_path, identity.source_ref
    );
    if identity.repository != plan.repository
        || identity.repository_id == 0
        || !is_full_sha(&identity.source_sha)
        || (plan.channel == PackageReleaseChannel::Stable
            && identity.workflow_sha != identity.source_sha)
        || identity.source_ref != format!("refs/heads/{}", plan.default_branch)
        || identity.workflow_path != plan.channel.workflow_path()
        || identity.workflow_ref != workflow_ref
        || identity.run_id == 0
        || identity.run_attempt == 0
        || identity.ci_run_id == 0
        || identity.ci_run_attempt == 0
        || match plan.channel {
            PackageReleaseChannel::Preview => {
                identity.event != plan.channel.event()
                    || identity.commit_count.is_none_or(|count| count == 0)
            }
            PackageReleaseChannel::Stable => {
                identity.event != plan.channel.event() || identity.commit_count.is_some()
            }
        }
    {
        return Err(internal("package_signer_workflow_identity_mismatch"));
    }
    Ok(())
}

pub(in crate::package_release_signer) fn release_version(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
) -> Result<(String, String, Option<u64>, Option<String>), OrchestratorError> {
    match plan.channel {
        PackageReleaseChannel::Preview => {
            let version = PreviewIdentity::derive(
                &plan.base_version,
                &identity.source_sha,
                identity
                    .commit_count
                    .ok_or_else(|| internal("package_signer_commit_count_missing"))?,
            )
            .map_err(|_| internal("package_signer_preview_version_invalid"))?;
            Ok((
                version.version,
                version.tag,
                Some(version.commit_count),
                Some(identity.source_sha[..7].to_owned()),
            ))
        }
        PackageReleaseChannel::Stable => Ok((
            plan.base_version.clone(),
            format!("v{}", plan.base_version),
            None,
            None,
        )),
    }
}

pub(in crate::package_release_signer) fn release_companions(
    plan: &PackageReleasePlan,
) -> Vec<RustBinaryReleaseCompanion> {
    plan.companions
        .iter()
        .map(|companion| RustBinaryReleaseCompanion {
            id: companion.id.clone(),
            path: companion.path.clone(),
            asset_name: companion.asset_name.clone(),
        })
        .collect()
}
