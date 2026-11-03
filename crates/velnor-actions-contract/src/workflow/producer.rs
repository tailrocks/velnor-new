// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_access::{CacheAccess, CacheWriter, SOURCES_PRODUCER_JOB_ID};
use super::cache_family::{TOFU_PROVIDERS_PROBE_MISS_CONDITION, TOFU_PROVIDERS_SAVE_CONDITION};
use super::crate_job::{TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX, TOOLS_PRODUCER_JOB_ID_PREFIX};
use super::producer_roles::{
    seed_role, sources_role, state_seal_build_role, state_seal_role, tofu_role, tools_role,
};
use super::state_seal::{
    STATE_CANARY_ARTIFACT_OUTPUT, STATE_CANARY_PRODUCER_JOB_ID, STATE_CANARY_SEAL_JOB_ID,
    STATE_CANARY_UNVERIFIED_TEST,
};
use super::step_identity::{RUST_VERIFIER_USES, ToolsRestoreForm};
use super::step_protocol::ProviderRestoreForm;
use super::step_tofu_prepare::valid_provider_prepare;
use super::{
    Job, PLAN_JOB_ID, PermissionLevel, Permissions, Step, StepKind, StepRole, TOFU_JOB_ID_PREFIX,
};
use crate::ContractError;

pub const DEPENDENCY_SEED_JOB_CONDITION: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";

pub const LANE_SURVIVAL_CONDITION: &str = "always()";

pub const TOOLS_PRODUCER_INSTALL_CONDITION: &str =
    "steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProducerKind {
    DependencySeed,
    ToolsIdentity,
    TofuProviders,
    SourcesProducer,
    StateSealBuild,
    StateSeal,
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
        } else if self
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::StateSealBuild))
        {
            Some(ProducerKind::StateSealBuild)
        } else if self
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::StateSealAttest))
        {
            Some(ProducerKind::StateSeal)
        } else if self.steps.iter().any(|step| {
            step.role == Some(StepRole::ToolsCacheRestore)
                && ToolsRestoreForm::of(&step.kind) == Some(ToolsRestoreForm::LookupOnly)
        }) {
            Some(ProducerKind::ToolsIdentity)
        } else if self.steps.iter().any(|step| {
            step.role == Some(StepRole::TofuProvidersSave)
                || (step.role == Some(StepRole::TofuProvidersRestore)
                    && ProviderRestoreForm::of(&step.kind) == Some(ProviderRestoreForm::LookupOnly))
        }) {
            Some(ProducerKind::TofuProviders)
        } else if self
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::CargoSourcesSave))
        {
            Some(ProducerKind::SourcesProducer)
        } else {
            None
        }
    }
}

pub fn validate_tofu_providers_producer(job: &Job, id: &str) -> Result<(), ContractError> {
    if job.producer_kind() != Some(ProducerKind::TofuProviders) {
        return Err(invalid(id));
    }
    let read_only = Permissions {
        contents: PermissionLevel::Read,
        ..Permissions::default()
    };
    validate_job(job, id, job.permissions.as_ref().unwrap_or(&read_only))
}

pub fn validate_sources_producer(job: &Job, id: &str) -> Result<(), ContractError> {
    if job.producer_kind() != Some(ProducerKind::SourcesProducer) {
        return Err(invalid(id));
    }
    let read_only = Permissions {
        contents: PermissionLevel::Read,
        ..Permissions::default()
    };
    validate_job(job, id, job.permissions.as_ref().unwrap_or(&read_only))
}

pub fn validate_tools_producer(job: &Job, id: &str) -> Result<(), ContractError> {
    if job.producer_kind() != Some(ProducerKind::ToolsIdentity) {
        return Err(invalid(id));
    }
    let read_only = Permissions {
        contents: PermissionLevel::Read,
        ..Permissions::default()
    };
    validate_job(job, id, job.permissions.as_ref().unwrap_or(&read_only))
}

pub(super) fn validate_job(
    job: &Job,
    id: &str,
    effective: &Permissions,
) -> Result<(), ContractError> {
    let kind = job.producer_kind();
    if id.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX) != (kind == Some(ProducerKind::ToolsIdentity))
        || (id == SOURCES_PRODUCER_JOB_ID) != (kind == Some(ProducerKind::SourcesProducer))
        || id.starts_with(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX)
            != (kind == Some(ProducerKind::TofuProviders))
        || (id == STATE_CANARY_PRODUCER_JOB_ID) != (kind == Some(ProducerKind::StateSealBuild))
        || (id == STATE_CANARY_SEAL_JOB_ID) != (kind == Some(ProducerKind::StateSeal))
    {
        return Err(invalid(id));
    }
    let Some(kind) = kind else {
        return job.cache_access.validate(id, job);
    };
    let cache_only = kind.condition_ok(job.condition.as_deref())
        && job.environment.is_none()
        && job.check_runner.is_none()
        && job.services.is_empty()
        && kind.outputs_ok(job)
        && kind.permissions_ok(effective)
        && job
            .steps
            .iter()
            .all(|step| !step.parallel && step.role.is_some_and(|role| kind.allows(role)))
        && kind.shape_ok(job);
    if !cache_only {
        return Err(invalid(id));
    }
    job.cache_access.validate(id, job)
}

fn invalid(id: &str) -> ContractError {
    ContractError::identity("job.producer", format!("mixed_or_invalid_producer:{id}"))
}

impl ProducerKind {
    #[must_use]
    pub(super) const fn cache_access(self) -> CacheAccess {
        match self {
            Self::DependencySeed | Self::StateSealBuild => CacheAccess::Read,
            Self::ToolsIdentity => CacheAccess::Writer(CacheWriter::ToolsProducer),
            Self::TofuProviders => CacheAccess::Writer(CacheWriter::TofuProvidersProducer),
            Self::SourcesProducer => CacheAccess::Writer(CacheWriter::SourcesProducer),
            Self::StateSeal => CacheAccess::Writer(CacheWriter::StateSeal),
        }
    }

    fn outputs_ok(self, job: &Job) -> bool {
        match self {
            Self::StateSealBuild => {
                job.outputs.len() == 1 && job.outputs.contains_key(STATE_CANARY_ARTIFACT_OUTPUT)
            }
            _ => job.outputs.is_empty(),
        }
    }

    fn permissions_ok(self, effective: &Permissions) -> bool {
        let others = [
            effective.pull_requests,
            effective.actions,
            effective.packages,
        ];
        if self == Self::StateSeal {
            return effective.id_token == PermissionLevel::Write
                && effective.attest == PermissionLevel::Write
                && effective.contents == PermissionLevel::None
                && effective.actions == PermissionLevel::Read
                && effective.pull_requests == PermissionLevel::None
                && effective.packages == PermissionLevel::None;
        }
        effective.id_token == PermissionLevel::None
            && effective.attest == PermissionLevel::None
            && effective.contents != PermissionLevel::Write
            && others.iter().all(|level| *level != PermissionLevel::Write)
    }

    fn allows(self, role: StepRole) -> bool {
        match self {
            Self::DependencySeed => seed_role(role),
            Self::ToolsIdentity => tools_role(role),
            Self::TofuProviders => tofu_role(role),
            Self::SourcesProducer => sources_role(role),
            Self::StateSealBuild => state_seal_build_role(role),
            Self::StateSeal => state_seal_role(role),
        }
    }

    fn condition_ok(self, condition: Option<&str>) -> bool {
        match self {
            Self::SourcesProducer => CacheWriter::SourcesProducer.admits(condition),
            Self::TofuProviders => CacheWriter::TofuProvidersProducer.admits(condition),
            Self::DependencySeed | Self::ToolsIdentity => {
                condition == Some(DEPENDENCY_SEED_JOB_CONDITION)
            }
            Self::StateSeal => CacheWriter::StateSeal.admits(condition),
            Self::StateSealBuild => {
                condition
                    == Some(
                        format!(
                            "{DEPENDENCY_SEED_JOB_CONDITION} && {STATE_CANARY_UNVERIFIED_TEST}"
                        )
                        .as_str(),
                    )
            }
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
            Self::SourcesProducer => {
                job.needs == [crate::workflow::PLAN_JOB_ID]
                    && count(StepRole::Checkout) == 1
                    && count(StepRole::CargoSourcesSave) == 1
                    && count(StepRole::CargoSourcesFetch) >= 1
                    && sources_order_ok(&job.steps)
            }
            Self::StateSealBuild => {
                job.needs == [PLAN_JOB_ID]
                    && count(StepRole::StateSealBuild) == 1
                    && count(StepRole::StateSealUpload) == 1
                    && job.steps.len() == 2
                    && job.steps[0].role == Some(StepRole::StateSealBuild)
            }
            Self::StateSeal => {
                job.needs == [PLAN_JOB_ID, STATE_CANARY_PRODUCER_JOB_ID]
                    && state_seal_order_ok(&job.steps)
            }
            Self::TofuProviders => {
                tofu_lane_needs_ok(&job.needs)
                    && count(StepRole::Checkout) == 1
                    && count(StepRole::MiseSetup) == 1
                    && count(StepRole::PreparePinnedTools) == 1
                    && count(StepRole::TofuProvidersRestore) == 1
                    && count(StepRole::TofuProvidersPrepare) == 1
                    && count(StepRole::TofuProviderUse) == 1
                    && count(StepRole::TofuProvidersSave) == 1
                    && tofu_order_ok(&job.steps)
            }
        }
    }
}

fn state_seal_order_ok(steps: &[Step]) -> bool {
    const ORDER: [StepRole; 5] = [
        StepRole::StateSealDownload,
        StepRole::StateSealSubject,
        StepRole::StateSealSave,
        StepRole::StateSealBind,
        StepRole::StateSealAttest,
    ];
    steps.len() == ORDER.len()
        && steps
            .iter()
            .zip(ORDER)
            .all(|(step, role)| step.role == Some(role))
}

fn tofu_lane_needs_ok(needs: &[String]) -> bool {
    match needs {
        [plan] => plan == "plan",
        [plan, previous] => {
            plan == "plan"
                && (previous.starts_with(TOFU_JOB_ID_PREFIX)
                    || previous.starts_with(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX))
        }
        _ => false,
    }
}

fn tofu_order_ok(steps: &[Step]) -> bool {
    let at = |role| steps.iter().position(|step| step.role == Some(role));
    let (Some(mise), Some(probe), Some(tools), Some(prepare), Some(init), Some(save)) = (
        at(StepRole::MiseSetup),
        at(StepRole::TofuProvidersRestore),
        at(StepRole::PreparePinnedTools),
        at(StepRole::TofuProvidersPrepare),
        at(StepRole::TofuProviderUse),
        at(StepRole::TofuProvidersSave),
    ) else {
        return false;
    };
    let gated = |index: usize| {
        steps[index].condition.as_deref() == Some(TOFU_PROVIDERS_PROBE_MISS_CONDITION)
    };
    mise < probe
        && probe < tools
        && tools < prepare
        && prepare < init
        && init + 1 == save
        && save + 1 == steps.len()
        && gated(tools)
        && gated(prepare)
        && gated(init)
        && steps[probe].condition.is_none()
        && steps[save].condition.as_deref() == Some(TOFU_PROVIDERS_SAVE_CONDITION)
        && ProviderRestoreForm::of(&steps[probe].kind) == Some(ProviderRestoreForm::LookupOnly)
        && matches!(&steps[probe].kind, StepKind::Action { with, .. }
            if with.get("path").is_some_and(|path| valid_provider_prepare(&steps[prepare].kind, path)))
}

fn sources_order_ok(steps: &[Step]) -> bool {
    let last_fetch = steps
        .iter()
        .rposition(|step| step.role == Some(StepRole::CargoSourcesFetch));
    let save = steps
        .iter()
        .position(|step| step.role == Some(StepRole::CargoSourcesSave));
    matches!((last_fetch, save), (Some(fetch), Some(save)) if fetch < save && save + 1 == steps.len())
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
