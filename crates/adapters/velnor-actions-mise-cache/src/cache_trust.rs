//! P08 trust, save, and quota policy: PR scoping, deltas, service data.
//!
//! The pinned MBX v1.6.0 action supports opt-in same-repo PR saving, but
//! Velnor keeps that option off: PRs restore the default-branch cache
//! read-only; any PR-branch save never promotes to trusted.
//! Fork PRs stay read-only. PR outputs never become trusted/release
//! evidence. Saves happen only for producer-successful useful deltas in
//! the allowed trust scope after writers finish. Cache-service errors
//! never fail verification nor permit skipped work. Quota/headroom come
//! from service data (`gh cache list --json`, `.../cache/usage`), never
//! a hardcoded limit: callers pass the limit in.

use velnor_actions_mise_core::error::MiseError;

/// Velnor does not opt in to the action's same-repository PR cache writes.
pub const MBX_PR_SAVE_OPTED_IN: bool = false;

/// True when a same-repo PR may save (action must support PR scoping).
///
/// Forks never save; non-PR events are governed by `save_allowed`.
#[must_use]
pub fn pr_save_allowed(action_supports_pr_save: bool, is_fork: bool, event: &str) -> bool {
    if is_fork {
        return false;
    }
    if event != "pull_request" {
        return false;
    }
    action_supports_pr_save
}

/// True when the run must treat caches read-only (forks always).
#[must_use]
pub fn is_read_only(is_fork: bool) -> bool {
    is_fork
}

/// True when `trust` marks PR (never trusted) evidence.
#[must_use]
pub fn pr_outputs_trusted(trust: &str) -> bool {
    trust == "trusted"
}

/// Save gate inputs: all four must hold before any cache save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "P08 names four independent save conditions; folding them hides the policy"
)]
pub struct SaveGate {
    /// The producer step/job succeeded.
    pub producer_passed: bool,
    /// The export carries a useful delta (nonempty, changed).
    pub useful_delta: bool,
    /// The current trust scope permits saving this namespace.
    pub trust_scope_ok: bool,
    /// Every concurrent writer finished before the save.
    pub writers_finished: bool,
}

/// Save only producer-successful useful deltas, in scope, writers done.
#[must_use]
pub fn save_after_success(gate: SaveGate) -> bool {
    gate.producer_passed && gate.useful_delta && gate.trust_scope_ok && gate.writers_finished
}

/// Authorize one trusted-layer save step; return its runtime gate.
///
/// Fails closed unless the save policy allows exactly protected pushes:
/// [`save_after_success`] must still require all four gate conditions,
/// [`crate::restore::save_decision`] must permit only the
/// trusted/push/passed combination, [`crate::cache::save_allowed`] must
/// agree, and the generator must not enable PR saves. The
/// returned condition is the `if:` gate the emitted save step carries;
/// policy drift fails generation instead of emitting a stale gate.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when the policy no longer matches
/// the emitted push-only gate.
pub fn authorize_trusted_save() -> Result<&'static str, MiseError> {
    authorize_trusted_save_for(MBX_PR_SAVE_OPTED_IN)
}

/// Authorize one trusted-layer save step under an explicit PR-save policy.
///
/// The generator policy arrives as a parameter so tests cover drift
/// rejection without flipping the production constant.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when the policy no longer matches
/// the emitted push-only gate.
pub fn authorize_trusted_save_for(pr_save_supported: bool) -> Result<&'static str, MiseError> {
    use crate::restore::{SaveInputs, save_decision};
    let open = SaveGate {
        producer_passed: true,
        useful_delta: true,
        trust_scope_ok: true,
        writers_finished: true,
    };
    if !save_after_success(open) {
        return Err(contract("save_gate_weakened"));
    }
    if pr_save_supported {
        return Err(contract("save_policy_drift:pr_save_supported"));
    }
    let allowed = SaveInputs {
        layer_trust: "trusted",
        event: "push",
        passed: true,
        unavailable: false,
        active_writer: false,
    };
    if save_decision(&allowed).is_err() {
        return Err(contract("save_policy_drift:push_denied"));
    }
    deny_non_push_variants(&allowed)?;
    for denied in [
        SaveInputs {
            passed: false,
            ..allowed
        },
        SaveInputs {
            unavailable: true,
            ..allowed
        },
        SaveInputs {
            active_writer: true,
            ..allowed
        },
    ] {
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:overpermissive"));
        }
    }
    if !crate::cache::save_allowed("trusted", "push", true)
        || crate::cache::save_allowed("trusted", "push", false)
    {
        return Err(contract("save_policy_drift:allowlist"));
    }
    let gate = velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    if !gate.contains("success()") {
        return Err(contract("save_gate_missing_success"));
    }
    Ok(gate)
}

/// Deny every non-push event and unknown trust spelling around `allowed`.
///
/// The allowlist is `push` only; `schedule`, `workflow_dispatch`, and
/// any future trigger stay denied, as do unknown trust spellings and
/// case variants. Both the decision model and the raw predicate must
/// agree, or the emitted gate is stale.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when any variant would save.
fn deny_non_push_variants(allowed: &crate::restore::SaveInputs<'_>) -> Result<(), MiseError> {
    use crate::restore::save_decision;
    for event in [
        "pull_request",
        "pull_request_target",
        "merge_group",
        "schedule",
        "workflow_dispatch",
        "workflow_call",
        "",
        "Push",
    ] {
        let denied = crate::restore::SaveInputs { event, ..*allowed };
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:event_allowed"));
        }
        if crate::cache::save_allowed("trusted", event, true) {
            return Err(contract("save_policy_drift:allowlist"));
        }
    }
    for layer_trust in ["", "untrusted", "prerelease"] {
        let denied = crate::restore::SaveInputs {
            layer_trust,
            ..*allowed
        };
        if save_decision(&denied).is_ok() {
            return Err(contract("save_policy_drift:trust_allowed"));
        }
    }
    Ok(())
}

/// Cache errors never turn successful verification into failure.
///
/// Returns false always: a cache failure leaves task success unchanged
/// and never permits skipping required work.
#[must_use]
pub fn cache_error_fails_verification(_cache_error: bool, _task_passed: bool) -> bool {
    false
}

/// Cache-service usage snapshot (from `gh` JSON, not hardcoded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheUsage {
    /// Active stored bytes across the repo.
    pub active_bytes: u64,
    /// Active cache entry count.
    pub count: u64,
}

/// Parse `gh cache list --json sizeInBytes` totals (no limit assumed).
///
/// Scans `"sizeInBytes":<digits>` pairs; no full JSON parser needed.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when the body is not a JSON array.
pub fn parse_service_usage(json: &str) -> Result<CacheUsage, MiseError> {
    let trimmed = json.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Err(contract("usage_not_array"));
    }
    let mut active_bytes: u64 = 0;
    let mut count: u64 = 0;
    let mut rest = trimmed;
    while let Some(at) = rest.find("sizeInBytes") {
        rest = &rest[at + "sizeInBytes".len()..];
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            break;
        }
        let skip = rest
            .find(&digits)
            .map_or(rest.len(), |at| at + digits.len());
        rest = &rest[skip.min(rest.len())..];
        match digits.parse::<u64>() {
            Ok(num) => {
                active_bytes = active_bytes.saturating_add(num);
                count += 1;
            }
            Err(_) => return Err(contract("usage_bad_size")),
        }
    }
    Ok(CacheUsage {
        active_bytes,
        count,
    })
}

/// Headroom from service-measured active bytes and a caller limit.
///
/// The limit arrives from service config/docs, never a literal here.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] when active exceeds limit.
pub fn headroom_bytes(active_bytes: u64, limit_bytes: u64) -> Result<u64, MiseError> {
    if active_bytes <= limit_bytes {
        Ok(limit_bytes - active_bytes)
    } else {
        Err(MiseError::InvalidStepInput {
            field: "cache_quota".to_owned(),
            value: format!("{active_bytes}>{limit_bytes}"),
        })
    }
}

/// Sequential-run cache report from live service data.
///
/// The single reporting path over [`parse_service_usage`],
/// [`headroom_bytes`], and [`stored_vs_transfer`]: one `gh cache list
/// --json` body plus a caller-supplied quota limit becomes the
/// stored/transfer/headroom figures recorded for R11. The generator
/// never calls this at render time (render is hermetic; service data
/// exists only after hosted runs) — the measurement fixture and the
/// documented `gh cache list` workflow are its callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheUsageReport {
    /// Active stored bytes across the repo (service-measured).
    pub active_bytes: u64,
    /// Active cache entry count.
    pub count: u64,
    /// Caller-supplied quota limit (service config/docs, never literal).
    pub limit_bytes: u64,
    /// Remaining quota (`limit_bytes - active_bytes`).
    pub headroom_bytes: u64,
    /// Stored bytes of the measured layer.
    pub stored_bytes: u64,
    /// Jobs restoring the layer this run.
    pub restoring_jobs: u64,
    /// Aggregate bytes every restoring runner downloads.
    pub aggregate_transfer_bytes: u64,
}

/// Report one layer against live service usage.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] for a non-array body and
/// [`MiseError::InvalidStepInput`] when active usage already exceeds
/// the quota limit (over-quota stays loud: the report never prints a
/// wrapped headroom).
pub fn summarize_cache_usage(
    json: &str,
    limit_bytes: u64,
    stored_bytes: u64,
    restoring_jobs: u64,
) -> Result<CacheUsageReport, MiseError> {
    let usage = parse_service_usage(json)?;
    let headroom = headroom_bytes(usage.active_bytes, limit_bytes)?;
    let (stored, transfer) = stored_vs_transfer(stored_bytes, restoring_jobs);
    Ok(CacheUsageReport {
        active_bytes: usage.active_bytes,
        count: usage.count,
        limit_bytes,
        headroom_bytes: headroom,
        stored_bytes: stored,
        restoring_jobs: restoring_jobs.max(1),
        aggregate_transfer_bytes: transfer,
    })
}

/// Stored vs unavoidable transfer: every restoring runner downloads.
///
/// Returns `(stored_bytes, aggregate_transfer_bytes)`.
#[must_use]
pub fn stored_vs_transfer(stored_bytes: u64, restoring_jobs: u64) -> (u64, u64) {
    (
        stored_bytes,
        stored_bytes.saturating_mul(restoring_jobs.max(1)),
    )
}

/// Contract-shaped rejection helper.
fn contract(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}
