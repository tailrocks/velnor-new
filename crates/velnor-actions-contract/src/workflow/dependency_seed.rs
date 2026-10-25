use super::{Job, PermissionLevel, Permissions, StepRole};
use crate::ContractError;

pub const DEPENDENCY_SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

impl Job {
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
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheSave
    )
}
