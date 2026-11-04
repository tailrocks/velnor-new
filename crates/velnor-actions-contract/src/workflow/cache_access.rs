// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::cache_family::SOURCES_PRODUCE_PLAN_TEST;
use super::producer::{DEPENDENCY_SEED_JOB_CONDITION, LANE_SURVIVAL_CONDITION, ProducerKind};
use super::state_seal::{STATE_CANARY_PRODUCER_JOB_ID, STATE_CANARY_UNVERIFIED_TEST};
use super::{Job, StepKind};
use crate::ContractError;

pub const WORKFLOW_CACHE_MODE: &str = "read";

pub const WRITER_CACHE_MODE: &str = "write";

pub const WRITE_ONLY_CACHE_MODE: &str = "write-only";

pub const SOURCES_PRODUCER_JOB_ID: &str = "sources-producer";

pub const QUALIFICATION_CACHE_WRITER_IDS: [&str; 4] = [
    "cache-scale-set",
    "empty-cache-hosted",
    "empty-cache-scale-set",
    "mbx-cache-write-hosted",
];

pub const QUALIFICATION_WRITER_GATE: &str =
    "github.ref != format('refs/heads/{0}', github.event.repository.default_branch)";

pub const QUALIFICATION_DEFAULT_BRANCH_GATE: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

pub const QUALIFICATION_CACHE_MODES: [&str; 3] = ["cache", "empty-cache", "mbx-cache-roundtrip"];

pub const QUALIFICATION_CACHE_GUARD_ID: &str = "cache-default-branch-guard";

pub const QUALIFICATION_CACHE_GUARD_ERROR: &str = "cache_qualification_requires_non_default_branch";

pub const WRITER_MISE_ENV: [(&str, &str); 2] = [("MISE_NO_CONFIG", "1"), ("MISE_NO_ENV", "1")];

const OWNED_ACTION_PREFIX: &str = "./.github/actions/velnor-";

const WRITER_USES: [&str; 4] = [
    "actions/checkout",
    "actions/cache/restore",
    "actions/cache/save",
    "jdx/mise-action",
];

const STATE_SEAL_USES: [&str; 3] = [
    "actions/download-artifact",
    "actions/attest",
    "actions/cache/save",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheAccess {
    Read,
    Writer(CacheWriter),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheWriter {
    ToolsProducer,
    TofuProvidersProducer,
    SourcesProducer,
    StateSeal,
}

impl CacheAccess {
    #[must_use]
    pub const fn mode(self) -> &'static str {
        match self {
            Self::Read => WORKFLOW_CACHE_MODE,
            Self::Writer(writer) => writer.mode(),
        }
    }

    #[must_use]
    pub const fn writer(self) -> Option<CacheWriter> {
        match self {
            Self::Read => None,
            Self::Writer(writer) => Some(writer),
        }
    }

    #[must_use]
    pub const fn is_read(self) -> bool {
        matches!(self, Self::Read)
    }

    pub(super) fn validate(self, id: &str, job: &Job) -> Result<(), ContractError> {
        let kind = job.producer_kind();
        let required = kind.map_or(Self::Read, ProducerKind::cache_access);
        let Self::Writer(writer) = self else {
            return if required == Self::Read {
                Ok(())
            } else {
                Err(invalid(id, "producer_without_write"))
            };
        };
        if job.steps.iter().any(|step| {
            step.parallel
                || matches!(
                    step.kind,
                    StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. }
                )
        }) {
            return Err(invalid(id, "runs_task_code"));
        }
        if required != self {
            return Err(invalid(id, "not_a_producer"));
        }
        if !writer.admits(job.condition.as_deref()) {
            return Err(invalid(id, "not_trusted_gate"));
        }
        for step in &job.steps {
            if let StepKind::Action { uses, .. } = &step.kind
                && !writer.allows_uses(uses)
            {
                return Err(invalid(id, &format!("third_party_uses:{uses}")));
            }
        }
        Ok(())
    }
}

impl CacheWriter {
    #[must_use]
    pub const fn mode(self) -> &'static str {
        match self {
            Self::StateSeal => WRITE_ONLY_CACHE_MODE,
            Self::ToolsProducer | Self::TofuProvidersProducer | Self::SourcesProducer => {
                WRITER_CACHE_MODE
            }
        }
    }

    #[must_use]
    pub fn condition(self) -> String {
        match self {
            Self::ToolsProducer | Self::TofuProvidersProducer => {
                DEPENDENCY_SEED_JOB_CONDITION.to_owned()
            }
            Self::SourcesProducer => {
                format!("{DEPENDENCY_SEED_JOB_CONDITION} && {SOURCES_PRODUCE_PLAN_TEST}")
            }
            Self::StateSeal => format!(
                "{DEPENDENCY_SEED_JOB_CONDITION} && {STATE_CANARY_UNVERIFIED_TEST} && needs.{STATE_CANARY_PRODUCER_JOB_ID}.result == 'success'"
            ),
        }
    }

    #[must_use]
    pub fn admits(self, condition: Option<&str>) -> bool {
        let Some(condition) = condition else {
            return false;
        };
        let gate = self.condition();
        condition == gate
            || (self == Self::TofuProvidersProducer
                && condition == format!("{LANE_SURVIVAL_CONDITION} && ({gate})"))
    }

    #[must_use]
    pub fn allows_uses(self, uses: &str) -> bool {
        let base = uses.split_once('@').map_or(uses, |(base, _)| base);
        if self == Self::StateSeal {
            return STATE_SEAL_USES.contains(&base);
        }
        WRITER_USES.contains(&base) || base.starts_with(OWNED_ACTION_PREFIX)
    }
}

fn invalid(id: &str, rule: &str) -> ContractError {
    ContractError::identity("job.cache_access", format!("cache_writer_{rule}:{id}"))
}
