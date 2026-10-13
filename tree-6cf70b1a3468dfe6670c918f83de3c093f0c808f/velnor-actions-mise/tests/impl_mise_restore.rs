//! Restore verification and miss-reason fallback cases.
use velnor_actions_contract::cachekey::MISS_REASONS;
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::MiseError;
use velnor_actions_mise::restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, fallback_for_error,
    verify_restored_task_result,
};

/// REUSE-5: evidence checks run present/digest/compat/owner/inputs, then
/// every declared output must be present and verified. Evidence builds
/// only from real observations via `verify` (no `intact()` shortcut).
#[test]
fn restore_verification_orders_evidence_then_outputs() {
    let verified = RestoreEvidence::verify([true; 5]);
    assert!(verified.check().is_ok());
    let cases = [
        (RestoreCheck::EntryPresent, MissReason::NO_ENTRY),
        (RestoreCheck::DigestMatches, MissReason::CACHE_CORRUPT),
        (
            RestoreCheck::CompatMatches,
            MissReason::COMPATIBILITY_MISMATCH,
        ),
        (RestoreCheck::OwnerMatches, MissReason::TRUST_SCOPE_MISMATCH),
        (RestoreCheck::InputsMatch, MissReason::INPUT_DIGEST_MISMATCH),
    ];
    for (check, reason) in cases {
        assert_eq!(verified.fail(check).check(), Err(reason), "evidence order");
    }
    assert!(
        RestoreEvidence::verify([false, true, true, true, true])
            .check()
            .is_err()
    );
    assert!(
        RestoreEvidence::verify([true, true, false, true, true])
            .check()
            .is_err()
    );
    let bytes = b"output bytes".to_vec();
    let observed = vec![("out.json".to_owned(), bytes.clone(), digest_b3(&bytes))];
    let declared = vec!["out.json".to_owned()];
    assert!(
        verify_restored_task_result("clippy", verified, &declared, &observed).is_ok(),
        "verified restore with verified outputs verifies"
    );
    assert_eq!(
        verify_restored_task_result("clippy", verified, &declared, &[]),
        Err(MissReason::TASK_RESULT_INCOMPLETE),
        "missing output is incomplete"
    );
    let tampered = vec![("out.json".to_owned(), b"other".to_vec(), digest_b3(&bytes))];
    assert_eq!(
        verify_restored_task_result("clippy", verified, &declared, &tampered),
        Err(MissReason::TASK_RESULT_INCOMPLETE),
        "mismatched output is incomplete"
    );
    assert_eq!(
        verify_restored_task_result(
            "clippy",
            verified.fail(RestoreCheck::EntryPresent),
            &declared,
            &observed,
        ),
        Err(MissReason::NO_ENTRY),
        "evidence runs before outputs"
    );
}

/// REUSE-6: the mise reason set is exactly the contract's 13 values.
#[test]
fn miss_reasons_cover_contract_set_exactly() {
    let mut ours: Vec<&str> = MissReason::ALL.iter().map(MissReason::as_str).collect();
    ours.sort_unstable();
    let mut theirs: Vec<&str> = MISS_REASONS.to_vec();
    theirs.sort_unstable();
    assert_eq!(ours, theirs, "mise reasons mirror the contract set");
    for reason in MissReason::ALL {
        assert_eq!(reason.to_string(), reason.as_str(), "display roundtrip");
    }
}

fn fallback_eligibility_cases() -> Vec<(MiseError, MissReason)> {
    let task = "clippy".to_owned();
    vec![
        (
            MiseError::CacheNotEligible {
                task: task.clone(),
                reason: "task_not_eligible".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::CacheNotEligible {
                task: task.clone(),
                reason: "task_result_incomplete".to_owned(),
            },
            MissReason::TASK_RESULT_INCOMPLETE,
        ),
        (
            MiseError::CacheNotEligible {
                task: task.clone(),
                reason: "forced_uncached".to_owned(),
            },
            MissReason::FORCED_UNCACHED,
        ),
        (
            MiseError::CacheNotEligible {
                task: task.clone(),
                reason: "bad_task_name".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::UnknownCacheMode {
                mode: "turbo".to_owned(),
            },
            MissReason::FORCED_UNCACHED,
        ),
        (
            MiseError::UnknownTool {
                tool: "nope".to_owned(),
            },
            MissReason::TOOL_MISSING,
        ),
        (
            MiseError::InvalidToolVersion {
                tool: "rust".to_owned(),
                version: "x".to_owned(),
            },
            MissReason::TOOL_MISSING,
        ),
    ]
}

fn fallback_artifact_cases() -> Vec<(MiseError, MissReason)> {
    vec![
        (
            MiseError::ArtifactNotFound {
                path: "x".to_owned(),
            },
            MissReason::NO_ENTRY,
        ),
        (
            MiseError::ArtifactUnreadable {
                path: "x".to_owned(),
                message: "denied".to_owned(),
            },
            MissReason::CACHE_UNAVAILABLE,
        ),
        (
            MiseError::DigestMismatch {
                expected: "a".to_owned(),
                actual: "b".to_owned(),
            },
            MissReason::CACHE_CORRUPT,
        ),
        (
            MiseError::InvalidDigest {
                value: "x".to_owned(),
                problem: "shape".to_owned(),
            },
            MissReason::CACHE_CORRUPT,
        ),
        (
            MiseError::InvalidUtf8 {
                program: "mise".to_owned(),
                stream: "stdout".to_owned(),
            },
            MissReason::CACHE_CORRUPT,
        ),
        (
            MiseError::ArtifactEscapesRoot {
                path: "../x".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
    ]
}

fn fallback_execution_cases() -> Vec<(MiseError, MissReason)> {
    vec![
        (
            MiseError::SpawnFailed {
                program: "mise".to_owned(),
                message: "noent".to_owned(),
            },
            MissReason::CACHE_UNAVAILABLE,
        ),
        (
            MiseError::NonZeroExit {
                program: "mise".to_owned(),
                code: Some(1),
                stderr: String::new(),
            },
            MissReason::CACHE_UNAVAILABLE,
        ),
        (
            MiseError::Contract {
                problem: "canonical".to_owned(),
            },
            MissReason::CACHE_UNAVAILABLE,
        ),
        (
            MiseError::EmptyCommand {
                program: "mise".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (MiseError::EmptyToolchain, MissReason::TASK_NOT_ELIGIBLE),
        (
            MiseError::ForbiddenPayload {
                program: "rustup".to_owned(),
                reason: "rustup_forbidden".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::GitVerbRejected {
                verb: "push".to_owned(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::InvalidManifestPath {
                path: String::new(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::InvalidNextestInput {
                field: "package".to_owned(),
                value: String::new(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::InvalidStepInput {
                field: "target".to_owned(),
                value: String::new(),
            },
            MissReason::TASK_NOT_ELIGIBLE,
        ),
        (
            MiseError::InvalidBaselineInput {
                field: "base_sha".to_owned(),
                value: "abc".to_owned(),
            },
            MissReason::CACHE_UNAVAILABLE,
        ),
    ]
}

/// REUSE-6: every mise error maps into the closed set, and fallback
/// always proceeds to execution.
#[test]
fn fallback_maps_every_error_and_executes() {
    let cases: Vec<(MiseError, MissReason)> = fallback_eligibility_cases()
        .into_iter()
        .chain(fallback_artifact_cases())
        .chain(fallback_execution_cases())
        .collect();
    for (error, reason) in &cases {
        assert_eq!(fallback_for_error(error), Ok(*reason), "map {error}");
    }
    assert_eq!(cases.len(), 24, "every variant mapped");
    let fallback = ReuseFallback::execute_with(MissReason::CACHE_UNAVAILABLE);
    assert_eq!(fallback.reason(), MissReason::CACHE_UNAVAILABLE);
    assert!(fallback.proceeds_to_execute(), "miss never fails");
}

/// P07-5: a cancelled or hung child never surfaces as a normal miss.
///
/// Cancel/timeout propagates as the typed error; only genuine backend
/// failures (e.g. `noent`) map to `cache_unavailable`.
#[test]
fn cancelled_child_never_maps_to_miss() {
    for message in ["cancelled", "timeout_after_secs:30", "timeout_after_secs:0"] {
        let error = MiseError::SpawnFailed {
            program: "mise".to_owned(),
            message: message.to_owned(),
        };
        assert_eq!(fallback_for_error(&error), Err(error.clone()), "{message}");
        assert!(velnor_actions_mise::command::is_cancel_or_timeout(&error));
    }
    let failure = MiseError::SpawnFailed {
        program: "mise".to_owned(),
        message: "noent".to_owned(),
    };
    assert_eq!(
        fallback_for_error(&failure),
        Ok(MissReason::CACHE_UNAVAILABLE)
    );
    let exit = MiseError::NonZeroExit {
        program: "mise".to_owned(),
        code: Some(1),
        stderr: String::new(),
    };
    assert_eq!(fallback_for_error(&exit), Ok(MissReason::CACHE_UNAVAILABLE));
}
