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

/// Exact identity a reuse must match: key, compatibility, trust scope.
#[derive(Debug, Clone)]
pub(crate) struct ExpectedReuseIdentity {
    /// Expected cache-key digest over the complete descriptor.
    pub(crate) cache_key: String,
    /// Expected compatibility digest.
    pub(crate) compatibility_id: String,
    /// Expected owner trust scope.
    pub(crate) owner_scope: String,
}

/// Observed restore outcome: entry key, compatibility, owner.
#[derive(Debug, Clone)]
pub(crate) struct ObservedRestoreMeta {
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
/// Zero-byte declared outputs are incomplete; malformed digests and
/// byte mismatches are corruption. Verification always hashes the
/// observed bytes; claimed digests are never trusted.
fn check_restored(declared: &[String], observed: &[ObservedOutput]) -> Result<(), MissReason> {
    for (_, bytes, digest) in observed {
        if bytes.is_empty() {
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
/// a trust anchor is incomplete evidence, never a pass.
fn check_verified(
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
) -> Result<(), MissReason> {
    let (Some(expected), Some(restore)) = (expected, restore) else {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    };
    if validate_digest(&expected.cache_key).is_err()
        || validate_digest(&expected.compatibility_id).is_err()
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
    Ok(())
}

/// Verify a reuse through every stage: presence, eligibility, restored
/// outputs, then the exact expected identity. Only a fully validated
/// restore passes; every unsafe case fails with a precise reason and
/// the caller executes instead (misses stay nonfatal optimizations).
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
) -> Result<(), MissReason> {
    check_presence(observed)?;
    check_eligibility(declared, descriptor)?;
    check_restored(declared, observed)?;
    check_verified(expected, restore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use velnor_actions_contract::digest_b3;

    /// Complete descriptor over one source, output, env entry, and tool.
    fn descriptor() -> CachedTaskDescriptor {
        CachedTaskDescriptor {
            task_name: "clippy".to_owned(),
            sources: vec!["Cargo.toml".to_owned()],
            outputs: vec!["out/report.json".to_owned()],
            command_inputs: Vec::new(),
            env: BTreeMap::from([("RUSTFLAGS".to_owned(), "--deny warnings".to_owned())]),
            tools: vec!["rust@1.98.1".to_owned()],
            dep_keys: Vec::new(),
        }
    }

    /// Expected identity plus matching restore metadata.
    fn identity() -> (ExpectedReuseIdentity, ObservedRestoreMeta) {
        let key = digest_b3(b"key");
        let compat = digest_b3(b"compat");
        (
            ExpectedReuseIdentity {
                cache_key: key.clone(),
                compatibility_id: compat.clone(),
                owner_scope: "trusted".to_owned(),
            },
            ObservedRestoreMeta {
                key,
                compat,
                owner: "trusted".to_owned(),
            },
        )
    }

    /// Declared outputs plus matching byte-verified observations.
    fn outputs() -> (Vec<String>, Vec<ObservedOutput>) {
        let bytes = b"report-bytes".to_vec();
        let digest = digest_b3(&bytes);
        (
            vec!["out/report.json".to_owned()],
            vec![("out/report.json".to_owned(), bytes, digest)],
        )
    }

    #[test]
    fn full_restore_verifies_and_stages_stay_separate() {
        let (declared, observed) = outputs();
        let (expected, restore) = identity();
        assert!(
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore)
            )
            .is_ok()
        );
        assert_eq!(
            verify_reused_pipeline(&declared, &[], None, None, None).expect_err("presence"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        assert_eq!(
            verify_reused_pipeline(&[], &observed, None, None, None).expect_err("eligibility"),
            MissReason::TASK_NOT_ELIGIBLE
        );
        let mut bare = descriptor();
        bare.outputs.clear();
        assert_eq!(
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&bare),
                Some(&expected),
                Some(&restore)
            )
            .expect_err("descriptor"),
            MissReason::TASK_NOT_ELIGIBLE
        );
        assert_eq!(
            verify_reused_pipeline(&declared, &observed, None, None, None).expect_err("trust gate"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
    }

    #[test]
    fn identity_mismatches_fail_with_precise_reasons() {
        let (declared, observed) = outputs();
        let (expected, mut restore) = identity();
        restore.key = digest_b3(b"other");
        assert_eq!(
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore)
            )
            .expect_err("key"),
            MissReason::INPUT_DIGEST_MISMATCH
        );
        let (_, mut restore) = identity();
        restore.compat = digest_b3(b"other");
        assert_eq!(
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore)
            )
            .expect_err("compat"),
            MissReason::COMPATIBILITY_MISMATCH
        );
        let (_, mut restore) = identity();
        restore.owner = "pr".to_owned();
        assert_eq!(
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore)
            )
            .expect_err("owner"),
            MissReason::TRUST_SCOPE_MISMATCH
        );
        let (_, restore) = identity();
        let mut tampered = observed.clone();
        tampered[0].1 = b"forged".to_vec();
        assert_eq!(
            verify_reused_pipeline(
                &declared,
                &tampered,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore)
            )
            .expect_err("bytes"),
            MissReason::CACHE_CORRUPT
        );
    }

    #[test]
    fn missing_and_malformed_outputs_fail() {
        let (declared, _) = outputs();
        let (expected, restore) = identity();
        let run = |observed: &[ObservedOutput]| {
            verify_reused_pipeline(
                &declared,
                observed,
                Some(&descriptor()),
                Some(&expected),
                Some(&restore),
            )
            .expect_err("output")
        };
        assert_eq!(
            run(&[("out/report.json".to_owned(), Vec::new(), digest_b3(b"x"))]),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        assert_eq!(
            run(&[(
                "out/report.json".to_owned(),
                b"report-bytes".to_vec(),
                "bogus".to_owned()
            )]),
            MissReason::CACHE_CORRUPT
        );
        let other = b"report-bytes".to_vec();
        let other_digest = digest_b3(&other);
        assert_eq!(
            run(&[("out/other.json".to_owned(), other, other_digest)]),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        assert_eq!(run(&[]), MissReason::TASK_RESULT_INCOMPLETE);
    }
}
