//! Task-reuse verification in separated stages (P04).
//!
//! Declared via `#[path]` from `wire_w2.rs` (no `lib.rs` edit).
//! Eligibility, Presence, Restored, and Verified are distinct checks
//! that run in order; only validated outputs plus the exact expected
//! identity construct a reuse. Presence short-circuits first so missing
//! restores report incomplete even for empty descriptors (merge-time
//! fail-closed).

use velnor_actions_contract::validate_digest;
use velnor_actions_mise::CachedTaskDescriptor;
use velnor_actions_mise::cache::verify_artifact_digest;
use velnor_actions_mise::restore::MissReason;
use velnor_actions_mise::restore_evidence::output_bytes_complete;

/// Exact identity a reuse must match: key, compatibility, trust scope,
/// and the recorded input digest the live digest must still equal.
#[derive(Debug, Clone)]
pub(crate) struct ExpectedReuseIdentity {
    /// Expected cache-key digest over the complete descriptor.
    pub(crate) cache_key: String,
    /// Expected compatibility digest.
    pub(crate) compatibility_id: String,
    /// Expected owner trust scope.
    pub(crate) owner_scope: String,
    /// Recorded input digest over task content at plan time.
    pub(crate) input_digest: String,
}

/// Observed restore outcome: task, entry key, compatibility, owner.
#[derive(Debug, Clone)]
pub(crate) struct ObservedRestoreMeta {
    /// Task the restore evidence was observed for.
    pub(crate) task_id: String,
    /// Observed entry cache-key digest.
    pub(crate) key: String,
    /// Observed entry compatibility digest.
    pub(crate) compat: String,
    /// Observed entry owner scope.
    pub(crate) owner: String,
}

/// One observed output: path, bytes, and claimed digest.
pub(crate) type ObservedOutput = (String, Vec<u8>, String);

/// Eligibility: declared outputs exist and the descriptor is complete.
///
/// Empty descriptors never pass: a descriptor without sources, outputs,
/// or tools cannot key a reuse. `None` (legacy merge-time path) defers
/// descriptor checks to the trust gate, which fails closed below.
fn check_eligibility(
    declared: &[String],
    descriptor: Option<&CachedTaskDescriptor>,
) -> Result<(), MissReason> {
    if declared.is_empty() {
        return Err(MissReason::TASK_NOT_ELIGIBLE);
    }
    let Some(descriptor) = descriptor else {
        return Ok(());
    };
    if descriptor.sources.is_empty() || descriptor.outputs.is_empty() || descriptor.tools.is_empty()
    {
        return Err(MissReason::TASK_NOT_ELIGIBLE);
    }
    descriptor
        .validate()
        .map_err(|_| MissReason::TASK_NOT_ELIGIBLE)
}

/// Presence: the restore produced a payload for the claimed key hit.
///
/// A key hit without payload is incomplete evidence, never a reuse.
fn check_presence(observed: &[ObservedOutput]) -> Result<(), MissReason> {
    if observed.is_empty() {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    }
    Ok(())
}

/// Restored: every observed output verifies against its claimed digest.
///
/// Zero-byte declared outputs are incomplete under the single P04 rule
/// in [`output_bytes_complete`], shared with the mise layer so both
/// layers always agree; malformed digests and byte mismatches are
/// corruption. Verification always hashes the observed bytes; claimed
/// digests are never trusted.
fn check_restored(declared: &[String], observed: &[ObservedOutput]) -> Result<(), MissReason> {
    for (_, bytes, digest) in observed {
        if !output_bytes_complete(bytes) {
            return Err(MissReason::TASK_RESULT_INCOMPLETE);
        }
        if validate_digest(digest).is_err() {
            return Err(MissReason::CACHE_CORRUPT);
        }
        if verify_artifact_digest(bytes, digest).is_err() {
            return Err(MissReason::CACHE_CORRUPT);
        }
    }
    for path in declared {
        if !observed.iter().any(|(name, _, _)| name == path) {
            return Err(MissReason::TASK_RESULT_INCOMPLETE);
        }
    }
    Ok(())
}

/// Verified: observed restore matches the exact expected identity.
///
/// Missing expectations or restore metadata fail closed: reuse without
/// a trust anchor is incomplete evidence, never a pass. The restore
/// must name this task, and the live input digest must still equal the
/// recorded plan-time digest: a same-path source edit flips the live
/// digest and rejects the reuse even when every other stage passes.
fn check_verified(
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
    task_id: &str,
    live_input_digest: Option<&str>,
) -> Result<(), MissReason> {
    let (Some(expected), Some(restore)) = (expected, restore) else {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    };
    if task_id.is_empty() || restore.task_id != task_id {
        return Err(MissReason::NO_ENTRY);
    }
    if validate_digest(&expected.cache_key).is_err()
        || validate_digest(&expected.compatibility_id).is_err()
        || validate_digest(&expected.input_digest).is_err()
        || expected.owner_scope.is_empty()
    {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    if validate_digest(&restore.key).is_err()
        || validate_digest(&restore.compat).is_err()
        || restore.owner.is_empty()
    {
        return Err(MissReason::CACHE_CORRUPT);
    }
    if restore.key != expected.cache_key {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    if restore.compat != expected.compatibility_id {
        return Err(MissReason::COMPATIBILITY_MISMATCH);
    }
    if restore.owner != expected.owner_scope {
        return Err(MissReason::TRUST_SCOPE_MISMATCH);
    }
    let Some(live) = live_input_digest else {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    };
    if validate_digest(live).is_err() || live != expected.input_digest {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    Ok(())
}

/// Verify a reuse through every stage: presence, eligibility, restored
/// outputs, then the exact expected identity plus the live input
/// binding. Only a fully validated restore passes; every unsafe case
/// fails with a precise reason and the caller executes instead (misses
/// stay nonfatal optimizations).
///
/// # Errors
///
/// Returns the first [`MissReason`] across the four stages.
pub(crate) fn verify_reused_pipeline(
    declared: &[String],
    observed: &[ObservedOutput],
    descriptor: Option<&CachedTaskDescriptor>,
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
    task_id: &str,
    live_input_digest: Option<&str>,
) -> Result<(), MissReason> {
    check_presence(observed)?;
    check_eligibility(declared, descriptor)?;
    check_restored(declared, observed)?;
    check_verified(expected, restore, task_id, live_input_digest)
}

#[cfg(test)]
mod tests;
