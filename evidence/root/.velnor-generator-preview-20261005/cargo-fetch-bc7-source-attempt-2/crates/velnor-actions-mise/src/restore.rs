//! Restore verification and miss-reason fallback for task-result reuse.
//!
//! Restore checks run in fixed order with ownership explicit, and every
//! mise failure maps to the closed miss-reason set. Fallback always
//! proceeds to execution: a cache miss never fails a task by itself.

use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::cache::verify_reused_outputs;
use crate::command::is_cancel_or_timeout;
use crate::error::MiseError;
use crate::restore_evidence::RestoreObservation;

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
    /// Build evidence from observed restore evidence: restored paths,
    /// bytes, and digests compared against recorded expectations (no
    /// `intact()` shortcut, no bare-bool constructor).
    ///
    /// Order follows [`RestoreCheck`]: presence, digest, compat, owner, inputs.
    #[must_use]
    pub fn verify(obs: &RestoreObservation) -> Self {
        Self {
            checks: obs.checks(),
        }
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

/// Map a mise failure to its miss reason, or propagate cancellation.
///
/// Cancel/timeout is checked BEFORE the mapping and returned as the
/// typed [`MiseError`]: a hung or cancelled child never degrades into
/// a normal cache miss (P07-5). All other failures map totally.
///
/// # Errors
///
/// Returns the original error when it is cancellation or timeout.
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
        | MiseError::InvalidBaselineInput { .. }
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
    })
}

/// Pinned-tool availability for the reuse decision (REUSE-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAvailability {
    /// Pinned tools passed qualification and are runnable.
    Ready,
    /// Task-cache feature present but Gate-6 qualification unpassed.
    Unqualified,
    /// Pinned tools failed qualification or are missing.
    Missing,
}

/// Honest Gate-6 qualification status (P04-10): task-cache NOT qualified.
///
/// No experiment has proven the opaque transport round-trips outputs
/// on the real backend. Probes report `Unqualified` until then.
pub const TASK_CACHE_QUALIFICATION_GATE: &str =
    "unpassed:gate6_mise_task_cache_requires_backend_proof";

/// Probe availability: `Ready` only with Gate-6 evidence, else explicit
/// `Unqualified` (default) or `Missing` when the probe itself failed.
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

/// Save-decision inputs for one cache layer (CACHE-2.x).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveInputs<'a> {
    /// Layer trust scope (`trusted` saves only on protected pushes).
    pub layer_trust: &'a str,
    /// Workflow event name.
    pub event: &'a str,
    /// Whether required checks passed.
    pub passed: bool,
    /// Cache backend unavailable.
    pub unavailable: bool,
    /// Another writer holds this layer (overlap guard).
    pub active_writer: bool,
}

/// Decide one cache save: allowed, or denied with its miss reason.
///
/// Denials never fail tasks: save failures leave success successful and
/// report `cache_write_disabled` or `cache_unavailable`.
///
/// # Errors
///
/// Returns the [`MissReason`] denying the save.
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

/// Whether a save carries a useful delta (CACHE-2.4).
///
/// A restore hit with no content change, a cancelled task, or an empty
/// result has nothing worth saving; the caller skips the save step.
#[must_use]
pub fn save_useful(unchanged_hit: bool, cancelled: bool, empty: bool) -> bool {
    !unchanged_hit && !cancelled && !empty
}

/// Whether another active writer already claims `layer`.
///
/// The orchestrator feeds its cache-ownership table; a claimed layer
/// denies the save through [`SaveInputs::active_writer`].
#[must_use]
pub fn writers_overlap(active_writers: &[String], layer: &str) -> bool {
    active_writers.iter().any(|writer| writer == layer)
}
