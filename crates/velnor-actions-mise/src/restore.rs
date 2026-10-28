// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::cache::verify_reused_outputs;
use crate::command::is_cancel_or_timeout;
use crate::error::MiseError;
use crate::restore_evidence::RestoreObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreCheck {
    EntryPresent,
    DigestMatches,
    CompatMatches,
    OwnerMatches,
    InputsMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreEvidence {
    checks: [bool; 5],
}

impl RestoreEvidence {
    #[must_use]
    pub fn verify(obs: &RestoreObservation) -> Self {
        Self {
            checks: obs.checks(),
        }
    }

    #[must_use]
    pub fn fail(mut self, check: RestoreCheck) -> Self {
        match check {
            RestoreCheck::EntryPresent => self.checks[0] = false,
            RestoreCheck::DigestMatches => self.checks[1] = false,
            RestoreCheck::CompatMatches => self.checks[2] = false,
            RestoreCheck::OwnerMatches => self.checks[3] = false,
            RestoreCheck::InputsMatch => self.checks[4] = false,
        }
        self
    }

    pub fn check(&self) -> Result<(), MissReason> {
        let [present, digest, compat, owner, inputs] = self.checks;
        if !present {
            return Err(MissReason::NO_ENTRY);
        }
        if !digest {
            return Err(MissReason::CACHE_CORRUPT);
        }
        if !compat {
            return Err(MissReason::COMPATIBILITY_MISMATCH);
        }
        if !owner {
            return Err(MissReason::TRUST_SCOPE_MISMATCH);
        }
        if !inputs {
            return Err(MissReason::INPUT_DIGEST_MISMATCH);
        }
        Ok(())
    }
}

pub fn verify_restored_task_result(
    task: &str,
    evidence: RestoreEvidence,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MissReason> {
    evidence.check()?;
    verify_reused_outputs(task, declared, observed).map_err(|_| MissReason::TASK_RESULT_INCOMPLETE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MissReason {
    value: &'static str,
}

impl MissReason {
    pub const NO_ENTRY: Self = Self { value: "no_entry" };
    pub const COMPATIBILITY_MISMATCH: Self = Self {
        value: "compatibility_mismatch",
    };
    pub const INPUT_DIGEST_MISMATCH: Self = Self {
        value: "input_digest_mismatch",
    };
    pub const TRUST_SCOPE_MISMATCH: Self = Self {
        value: "trust_scope_mismatch",
    };
    pub const CACHE_UNAVAILABLE: Self = Self {
        value: "cache_unavailable",
    };
    pub const CACHE_CORRUPT: Self = Self {
        value: "cache_corrupt",
    };
    pub const CACHE_EXPIRED: Self = Self {
        value: "cache_expired",
    };
    pub const CACHE_WRITE_DISABLED: Self = Self {
        value: "cache_write_disabled",
    };
    pub const TASK_NOT_ELIGIBLE: Self = Self {
        value: "task_not_eligible",
    };
    pub const TASK_RESULT_INCOMPLETE: Self = Self {
        value: "task_result_incomplete",
    };
    pub const FORCED_UNCACHED: Self = Self {
        value: "forced_uncached",
    };
    pub const TOOL_MISSING: Self = Self {
        value: "tool_missing",
    };
    pub const SOURCE_MISSING: Self = Self {
        value: "source_missing",
    };
    pub const ALL: [Self; 13] = [
        Self::NO_ENTRY,
        Self::COMPATIBILITY_MISMATCH,
        Self::INPUT_DIGEST_MISMATCH,
        Self::TRUST_SCOPE_MISMATCH,
        Self::CACHE_UNAVAILABLE,
        Self::CACHE_CORRUPT,
        Self::CACHE_EXPIRED,
        Self::CACHE_WRITE_DISABLED,
        Self::TASK_NOT_ELIGIBLE,
        Self::TASK_RESULT_INCOMPLETE,
        Self::FORCED_UNCACHED,
        Self::TOOL_MISSING,
        Self::SOURCE_MISSING,
    ];

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        self.value
    }
}

impl Display for MissReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseFallback {
    reason: MissReason,
}

impl ReuseFallback {
    #[must_use]
    pub fn execute_with(reason: MissReason) -> Self {
        Self { reason }
    }

    #[must_use]
    pub fn reason(self) -> MissReason {
        self.reason
    }

    #[must_use]
    pub fn proceeds_to_execute(self) -> bool {
        true
    }
}

pub fn fallback_for_error(error: &MiseError) -> Result<MissReason, MiseError> {
    if is_cancel_or_timeout(error) {
        return Err(error.clone());
    }
    Ok(match error {
        MiseError::CacheNotEligible { reason, .. } => match reason.as_str() {
            "task_result_incomplete" => MissReason::TASK_RESULT_INCOMPLETE,
            "forced_uncached" => MissReason::FORCED_UNCACHED,
            _ => MissReason::TASK_NOT_ELIGIBLE,
        },
        MiseError::ArtifactNotFound { .. } => MissReason::NO_ENTRY,
        MiseError::DigestMismatch { .. }
        | MiseError::InvalidDigest { .. }
        | MiseError::InvalidUtf8 { .. } => MissReason::CACHE_CORRUPT,
        MiseError::ArtifactUnreadable { .. }
        | MiseError::SpawnFailed { .. }
        | MiseError::NonZeroExit { .. }
        | MiseError::Contract { .. } => MissReason::CACHE_UNAVAILABLE,
        MiseError::UnknownCacheMode { .. } => MissReason::FORCED_UNCACHED,
        MiseError::UnknownTool { .. }
        | MiseError::InvalidToolVersion { .. }
        | MiseError::MissingToolPin { .. } => MissReason::TOOL_MISSING,
        MiseError::EmptyCommand { .. }
        | MiseError::EmptyToolchain
        | MiseError::ForbiddenPayload { .. }
        | MiseError::GitVerbRejected { .. }
        | MiseError::InvalidManifestPath { .. }
        | MiseError::InvalidNextestInput { .. }
        | MiseError::InvalidStepInput { .. }
        | MiseError::ArtifactEscapesRoot { .. } => MissReason::TASK_NOT_ELIGIBLE,
        MiseError::OperatorToolchainGuard { .. } => return Err(error.clone()),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAvailability {
    Ready,
    Unqualified,
    Missing,
}

pub const TASK_CACHE_QUALIFICATION_GATE: &str =
    "unpassed:gate6_mise_task_cache_requires_backend_proof";

#[must_use]
pub fn probe_tool_availability(qualified: bool, probe_failed: bool) -> ToolAvailability {
    if probe_failed {
        ToolAvailability::Missing
    } else if qualified {
        ToolAvailability::Ready
    } else {
        ToolAvailability::Unqualified
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveInputs<'a> {
    pub layer_trust: &'a str,
    pub event: &'a str,
    pub passed: bool,
    pub unavailable: bool,
    pub active_writer: bool,
}

pub fn save_decision(inputs: &SaveInputs<'_>) -> Result<(), MissReason> {
    if inputs.unavailable {
        return Err(MissReason::CACHE_UNAVAILABLE);
    }
    if inputs.active_writer {
        return Err(MissReason::CACHE_WRITE_DISABLED);
    }
    if !crate::cache::save_allowed(inputs.layer_trust, inputs.event, inputs.passed) {
        return Err(MissReason::CACHE_WRITE_DISABLED);
    }
    Ok(())
}

#[must_use]
pub fn save_useful(unchanged_hit: bool, cancelled: bool, empty: bool) -> bool {
    !unchanged_hit && !cancelled && !empty
}

#[must_use]
pub fn writers_overlap(active_writers: &[String], layer: &str) -> bool {
    active_writers.iter().any(|writer| writer == layer)
}
