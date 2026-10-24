//! Closed cache-only job role; only validated dependency seeds may be optional.
use super::{Job, PermissionLevel, Permissions, StepRole};
use crate::ContractError;

/// Only protected pushes to the repository's default branch write seed bundles.
pub const DEPENDENCY_SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

impl Job {
    /// Whether this job carries the typed dependency-seed build role.
    ///
    /// Workflow validation must succeed before this classification grants any
    /// optional-failure behavior; mixed execution or report roles are rejected.
    #[must_use]
    pub fn is_dependency_seed(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.role == Some(StepRole::MbxSeedBuild))
    }
}

pub(super) fn validate_job(
    job: &Job,
    id: &str,
    effective: &Permissions,
) -> Result<(), ContractError> {
    if !job.is_dependency_seed() {
        return Ok(());
    }
    let cache_only = job.condition.as_deref() == Some(DEPENDENCY_SEED_JOB_CONDITION)
        && job.needs == ["plan"]
        && job.environment.is_none()
        && job.check_runner.is_none()
        && job.services.is_empty()
        && effective.id_token == PermissionLevel::None
        && [
            effective.contents,
            effective.pull_requests,
            effective.actions,
            effective.packages,
        ]
        .into_iter()
        .all(|level| level != PermissionLevel::Write)
        && job
            .steps
            .iter()
            .all(|step| !step.parallel && step.role.is_some_and(cache_role))
        && valid_preseed_consumer_steps(&job.steps)
        && job
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::MbxSeedBuild))
            .count()
            == 3
        && job
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::MbxProducerMark))
            .count()
            == 1;
    if cache_only {
        Ok(())
    } else {
        Err(ContractError::identity(
            "job.dependency_seed",
            format!("mixed_or_invalid_seed:{id}"),
        ))
    }
}

/// Preseed consumers are allowed only as the exact verified trio emitted by
/// the renderer. Their output must exist before any helper-dependent cache,
/// private-source, or MBX step can run.
fn valid_preseed_consumer_steps(steps: &[super::Step]) -> bool {
    use StepRole::{PreseedDownload, PreseedStage, PreseedVerifyManifest};

    let positions = [PreseedDownload, PreseedVerifyManifest, PreseedStage].map(|role| {
        steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.role == Some(role))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    });
    if positions.iter().all(Vec::is_empty) {
        return true;
    }
    if positions.iter().any(|matches| matches.len() != 1)
        || steps
            .iter()
            .any(|step| step.role == Some(StepRole::AcquireVelnor))
    {
        return false;
    }

    let [download, verify, stage] = positions.map(|matches| matches[0]);
    let Some(checkout) = steps
        .iter()
        .position(|step| step.role == Some(StepRole::Checkout))
    else {
        return false;
    };
    if !(checkout < download && download < verify && verify < stage) {
        return false;
    }

    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_preseed_helper_consumer(step.role))
        .all(|(index, _)| stage < index)
}

fn is_preseed_helper_consumer(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::CacheTarLaunchers
                | StepRole::PrivateTokenMint
                | StepRole::PrivateFetch
                | StepRole::PrivateRevoke
                | StepRole::MbxPreflight
                | StepRole::MbxCache
                | StepRole::MbxVersionCheck
                | StepRole::MbxWorkspaceCleanup
                | StepRole::MbxBundleKey
                | StepRole::MbxBundleRestore
                | StepRole::MbxBundleImport
                | StepRole::MbxSeedBuild
                | StepRole::MbxProducerMark
                | StepRole::MbxBundleExport
                | StepRole::MbxBundleSave
        )
    )
}

fn cache_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::MiseSetup
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::AcquireVelnor
            | StepRole::CargoSourcesRestore
            | StepRole::CargoSourcesFetch
            | StepRole::MbxPreflight
            | StepRole::MbxCache
            | StepRole::MbxVersionCheck
            | StepRole::MbxWorkspaceCleanup
            | StepRole::MbxBundleKey
            | StepRole::MbxBundleRestore
            | StepRole::MbxBundleImport
            | StepRole::MbxProducerMark
            | StepRole::MbxSeedBuild
            | StepRole::MbxBundleExport
            | StepRole::MbxBundleSave
            | StepRole::PrivateTokenMint
            | StepRole::PrivateFetch
            | StepRole::PrivateRevoke
            | StepRole::PreseedDownload
            | StepRole::PreseedVerifyManifest
            | StepRole::PreseedStage
            // Install the pinned tool used to bootstrap the cache-only job.
            | StepRole::ToolSeed
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheSave
            // The pinned archive adapter only enables the cache steps below.
            | StepRole::CacheTarLaunchers
    )
}

#[cfg(test)]
#[path = "dependency_seed_tests.rs"]
mod tests;
