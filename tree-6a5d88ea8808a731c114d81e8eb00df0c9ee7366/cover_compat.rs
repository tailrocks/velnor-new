//! Baseline compatibility derivation shared by lookup and publish.
//!
//! The manifest `compatibility_id` binds the execution shape both sides
//! can derive from the plan alone: the runner label plus every
//! obligation's `(task_id, task_digest)` pair, sorted. Task digests bind
//! command plus toolchain, never source content, so a source edit keeps
//! compatibility while per-task entries still gate coverage exactly; a
//! toolchain bump or a changed task set misses to full execution.
//!
//! The manifest `artifact_id` is the numeric fingerprint of the derived
//! artifact name, never the service-assigned artifact ID: the publisher
//! writes the manifest before uploading, so no manifest can carry an ID
//! the service assigns at upload time. The API listing still proves the
//! exact artifact exists unexpired in the exact run before any download.

use velnor_actions_contract::{Plan, canonical_json_bytes, digest_b3};

/// Manifest compatibility for one plan's obligation set.
///
/// Sorted `(task_id, task_digest)` pairs plus the runner label, hashed
/// over canonical JSON. Deterministic across runs for identical plans.
/// # Errors
///
/// Returns the reason when canonical encoding fails.
pub(crate) fn baseline_compat_for_plan(plan: &Plan) -> Result<String, String> {
    let mut pairs: Vec<(&str, &str)> = plan
        .obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect();
    pairs.sort_unstable();
    let bytes = canonical_json_bytes(&serde_json::json!({
        "label": plan.runner.label,
        "tasks": pairs,
    }))
    .map_err(|_| "compat_encode_failed".to_owned())?;
    Ok(digest_b3(&bytes))
}

/// Manifest-assigned numeric fingerprint of one derived artifact name.
///
/// First eight canonical-digest bytes as big-endian `u64`, mapped off
/// zero so the manifest's identified-run field stays meaningful.
/// Deterministic: lookup, publish, and validation derive the same
/// value from the name without contacting the service.
#[must_use]
pub fn baseline_artifact_numeric_id(name: &str) -> u64 {
    let digest = digest_b3(name.as_bytes());
    let hex = digest
        .strip_prefix("b3-")
        .and_then(|hex| hex.get(..16))
        .unwrap_or("");
    u64::from_str_radix(hex, 16).unwrap_or(0).max(1)
}

#[cfg(test)]
#[path = "cover_compat_tests.rs"]
mod cover_compat_tests;
