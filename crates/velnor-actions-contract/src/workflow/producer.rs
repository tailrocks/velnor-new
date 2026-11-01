// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::crate_job::TOOLS_PRODUCER_JOB_ID_PREFIX;
use super::step_identity::{RUST_VERIFIER_USES, ToolsRestoreForm};
use super::{Job, PermissionLevel, Permissions, Step, StepKind, StepRole};
use crate::ContractError;

pub const DEPENDENCY_SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

pub const TOOLS_PRODUCER_INSTALL_CONDITION: &str =
    "steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProducerKind {
    DependencySeed,
    ToolsIdentity,
}

impl Job {
    #[must_use]
    pub fn is_dependency_seed(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.role == Some(StepRole::MbxSeedBuild))
    }

    #[must_use]
    pub fn producer_kind(&self) -> Option<ProducerKind> {
        if self.is_dependency_seed() {
            Some(ProducerKind::DependencySeed)
        } else if self.steps.iter().any(|step| {
            step.role == Some(StepRole::ToolsCacheRestore)
                && ToolsRestoreForm::of(&step.kind) == Some(ToolsRestoreForm::LookupOnly)
        }) {
            Some(ProducerKind::ToolsIdentity)
        } else {
            None
        }
    }
}

pub(super) fn validate_job(
    job: &Job,
    id: &str,
    effective: &Permissions,
) -> Result<(), ContractError> {
    let kind = job.producer_kind();
    if id.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX) != (kind == Some(ProducerKind::ToolsIdentity)) {
        return Err(invalid(id));
    }
    let Some(kind) = kind else {
        return Ok(());
    };
    let cache_only = job.condition.as_deref() == Some(DEPENDENCY_SEED_JOB_CONDITION)
        && job.environment.is_none()
        && job.check_runner.is_none()
        && job.services.is_empty()
        && job.outputs.is_empty()
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
            .all(|step| !step.parallel && step.role.is_some_and(|role| kind.allows(role)))
        && kind.shape_ok(job);
    if cache_only { Ok(()) } else { Err(invalid(id)) }
}

fn invalid(id: &str) -> ContractError {
    ContractError::identity("job.producer", format!("mixed_or_invalid_producer:{id}"))
}

impl ProducerKind {
    fn allows(self, role: StepRole) -> bool {
        match self {
            Self::DependencySeed => seed_role(role),
            Self::ToolsIdentity => tools_role(role),
        }
    }

    fn shape_ok(self, job: &Job) -> bool {
        let count = |role| {
            job.steps
                .iter()
                .filter(|step| step.role == Some(role))
                .count()
        };
        match self {
            Self::DependencySeed => {
                job.needs == ["plan"]
                    && count(StepRole::MbxSeedBuild) == 3
                    && count(StepRole::MbxProducerMark) == 1
            }
            Self::ToolsIdentity => {
                job.needs.is_empty()
                    && count(StepRole::Checkout) == 1
                    && count(StepRole::MiseSetup) == 1
                    && count(StepRole::ToolsCacheIdentity) == 1
                    && count(StepRole::ToolsCacheRestore) == 1
                    && count(StepRole::ToolsCacheScrub) == 1
                    && count(StepRole::ToolsCacheSave) == 1
                    && count(StepRole::PreparePinnedTools) >= 1
                    && tools_order_ok(&job.steps)
            }
        }
    }
}

fn tools_order_ok(steps: &[Step]) -> bool {
    let at = |role| steps.iter().position(|step| step.role == Some(role));
    let (Some(checkout), Some(mise), Some(identity), Some(probe), Some(scrub), Some(save)) = (
        at(StepRole::Checkout),
        at(StepRole::MiseSetup),
        at(StepRole::ToolsCacheIdentity),
        at(StepRole::ToolsCacheRestore),
        at(StepRole::ToolsCacheScrub),
        at(StepRole::ToolsCacheSave),
    ) else {
        return false;
    };
    checkout < mise
        && mise < identity
        && identity < probe
        && probe < scrub
        && save + 1 == steps.len()
        && steps.iter().enumerate().all(|(index, step)| {
            if step.role == Some(StepRole::ToolsCacheScrub) || is_install_role(step.role) {
                index >= scrub
                    && index < save
                    && step.condition.as_deref() == Some(TOOLS_PRODUCER_INSTALL_CONDITION)
                    && installed_verifier_only(step)
            } else {
                true
            }
        })
}

fn is_install_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreparePinnedTools
                | StepRole::PrepareRustComponents
                | StepRole::VerifyRustManifest
                | StepRole::VerifyJactionlint
        )
    )
}

fn installed_verifier_only(step: &Step) -> bool {
    match (&step.role, &step.kind) {
        (Some(StepRole::VerifyRustManifest), StepKind::Action { uses, .. }) => {
            uses == RUST_VERIFIER_USES[1]
        }
        _ => true,
    }
}

fn tools_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::MiseSetup
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheScrub
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::VerifyJactionlint
            | StepRole::ToolsCacheSave
    )
}

fn seed_role(role: StepRole) -> bool {
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
            | StepRole::MbxBundleRecord
            | StepRole::PrivateTokenMint
            | StepRole::PrivateFetch
            | StepRole::PrivateRevoke
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheSave
    )
}
