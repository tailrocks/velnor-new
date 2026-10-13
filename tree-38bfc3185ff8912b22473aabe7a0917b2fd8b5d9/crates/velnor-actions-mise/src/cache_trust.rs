//! P08 trust, save, and quota policy: PR scoping, deltas, service data.
//!
//! Same-repo PR saving only when the pinned action supports a PR-scoped
//! policy (pinned MBX v1.5.0 does not: PRs restore the default-branch
//! cache read-only; any PR-branch save never promotes to trusted).
//! Fork PRs stay read-only. PR outputs never become trusted/release
//! evidence. Saves happen only for producer-successful useful deltas in
//! the allowed trust scope after writers finish. Cache-service errors
//! never fail verification nor permit skipped work. Quota/headroom come
//! from service data (`gh cache list --json`, `.../cache/usage`), never
//! a hardcoded limit: callers pass the limit in.

use crate::error::MiseError;

/// Pinned MBX action supports PR-scoped save policy (v1.5.0: no).
pub const MBX_PR_SAVE_SUPPORTED: bool = false;

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
