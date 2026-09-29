//! Restore verification and miss-reason fallback for task-result reuse.
//!
//! Restore checks run in fixed order with ownership explicit, and every
//! mise failure maps to the closed miss-reason set. Fallback always
//! proceeds to execution: a cache miss never fails a task by itself.

use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::cache::{TaskCacheMode, verify_reused_outputs};
use crate::error::MiseError;
use crate::reuse::{ReuseGrant, ReuseQualification};

/// One ordered restore check; the first failure wins (REUSE-5).
/// Ownership is explicit: the archive owner must match the trust scope
/// before any byte is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreCheck {
    /// Archive entry exists.
    EntryPresent,
    /// Entry digest matches.
    DigestMatches,
    /// Compatibility id matches.
    CompatMatches,
    /// Archive owner matches the trust scope.
    OwnerMatches,
    /// Input digest matches.
    InputsMatch,
}

/// Ordered restore outcomes in [`RestoreCheck`] order: presence, digest,
/// compatibility, ownership, inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreEvidence {
    /// Check outcomes in [`RestoreCheck`] order.
    checks: [bool; 5],
}

impl RestoreEvidence {
    /// All checks passing.
    #[must_use]
    pub fn intact() -> Self {
        Self { checks: [true; 5] }
    }

    /// Record one failing check.
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

    /// First failing check as a precise miss reason.
    ///
    /// # Errors
    ///
    /// Returns the [`MissReason`] for the first failing check.
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

/// Verify a restored result: evidence first, then every declared output
/// present with a matching digest. A hit is `reused` only when all of
/// these pass; anything else is a precise miss reason, never success.
///
/// # Errors
///
/// Returns the first [`MissReason`]: an evidence reason, or
/// `task_result_incomplete` when an output is missing or mismatched.
pub fn verify_restored_task_result(
    task: &str,
    evidence: RestoreEvidence,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MissReason> {
    evidence.check()?;
    verify_reused_outputs(task, declared, observed).map_err(|_| MissReason::TASK_RESULT_INCOMPLETE)
}

/// Closed miss-reason set mirroring the contract's 13 `miss_reason`
/// values (REUSE-6). Only these tokens are ever reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MissReason {
    /// Reason token as reported.
    value: &'static str,
}

impl MissReason {
    /// No archive entry exists.
    pub const NO_ENTRY: Self = Self { value: "no_entry" };
    /// Compatibility id differs.
    pub const COMPATIBILITY_MISMATCH: Self = Self {
        value: "compatibility_mismatch",
    };
    /// Input digest differs.
    pub const INPUT_DIGEST_MISMATCH: Self = Self {
        value: "input_digest_mismatch",
    };
    /// Archive owner is outside the trust scope.
    pub const TRUST_SCOPE_MISMATCH: Self = Self {
        value: "trust_scope_mismatch",
    };
    /// Cache backend or task-cache run failed.
    pub const CACHE_UNAVAILABLE: Self = Self {
        value: "cache_unavailable",
    };
    /// Entry bytes fail verification.
    pub const CACHE_CORRUPT: Self = Self {
        value: "cache_corrupt",
    };
    /// Entry expired.
    pub const CACHE_EXPIRED: Self = Self {
        value: "cache_expired",
    };
    /// Save path is disabled for this event.
    pub const CACHE_WRITE_DISABLED: Self = Self {
        value: "cache_write_disabled",
    };
    /// Task kind or request cannot reuse.
    pub const TASK_NOT_ELIGIBLE: Self = Self {
        value: "task_not_eligible",
    };
    /// Restored outputs are missing or mismatched.
    pub const TASK_RESULT_INCOMPLETE: Self = Self {
        value: "task_result_incomplete",
    };
    /// Reuse forcibly disabled (release, `Off`).
    pub const FORCED_UNCACHED: Self = Self {
        value: "forced_uncached",
    };
    /// Pinned tool is missing.
    pub const TOOL_MISSING: Self = Self {
        value: "tool_missing",
    };
    /// Declared source is missing.
    pub const SOURCE_MISSING: Self = Self {
        value: "source_missing",
    };
    /// All 13 reasons in contract order.
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

    /// Reason token as reported.
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

/// Fallback decision: discard the cache path and execute, reporting the
/// reason. A cache miss never fails a task by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseFallback {
    /// Miss reason to report.
    reason: MissReason,
}

impl ReuseFallback {
    /// Fall back to execution, reporting `reason`.
    #[must_use]
    pub fn execute_with(reason: MissReason) -> Self {
        Self { reason }
    }

    /// Miss reason to report.
    #[must_use]
    pub fn reason(self) -> MissReason {
        self.reason
    }

    /// Fallback always proceeds to execution; misses never fail tasks.
    #[must_use]
    pub fn proceeds_to_execute(self) -> bool {
        true
    }
}

/// Map any mise failure to the miss reason reported while executing
/// anyway (REUSE-6). The mapping is total over [`MiseError`].
#[must_use]
pub fn fallback_for_error(error: &MiseError) -> MissReason {
    match error {
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
        MiseError::UnknownTool { .. } | MiseError::InvalidToolVersion { .. } => {
            MissReason::TOOL_MISSING
        }
        MiseError::EmptyCommand { .. }
        | MiseError::EmptyToolchain
        | MiseError::ForbiddenPayload { .. }
        | MiseError::GitVerbRejected { .. }
        | MiseError::InvalidManifestPath { .. }
        | MiseError::InvalidNextestInput { .. }
        | MiseError::InvalidStepInput { .. }
        | MiseError::ArtifactEscapesRoot { .. } => MissReason::TASK_NOT_ELIGIBLE,
    }
}

/// Pinned-tool availability for the reuse decision (REUSE-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAvailability {
    /// Pinned tools passed qualification and are runnable.
    Ready,
    /// Pinned tools failed qualification or are missing.
    Missing,
}

/// Reuse plan: reuse under a grant, or execute with a fallback reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReusePlan {
    /// Reuse the cached result under this grant.
    Reuse(ReuseGrant),
    /// Execute without reuse, reporting this fallback.
    Execute(ReuseFallback),
}

/// Decide reuse for one qualification: missing tools, release paths, and
/// unqualified tasks execute without reuse and report their reason.
#[must_use]
pub fn plan_reuse(
    availability: ToolAvailability,
    qualification: &ReuseQualification,
    mode: TaskCacheMode,
) -> ReusePlan {
    if availability != ToolAvailability::Ready {
        return ReusePlan::Execute(ReuseFallback::execute_with(MissReason::CACHE_UNAVAILABLE));
    }
    match qualification.check(mode) {
        Ok(grant) => ReusePlan::Reuse(grant),
        Err(error) => ReusePlan::Execute(ReuseFallback::execute_with(fallback_for_error(&error))),
    }
}
