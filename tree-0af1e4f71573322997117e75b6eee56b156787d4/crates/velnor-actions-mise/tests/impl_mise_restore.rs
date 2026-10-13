//! Restore verification and miss-reason fallback cases.
use velnor_actions_contract::cachekey::MISS_REASONS;
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::MiseError;
use velnor_actions_mise::restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, fallback_for_error,
    verify_restored_task_result,
};

/// REUSE-5: evidence checks run present/digest/compat/owner/inputs, then
/// every declared output must be present and verified.
#[test]
fn restore_verification_orders_evidence_then_outputs() {
    let intact = RestoreEvidence::intact();
    assert!(intact.check().is_ok());
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
        assert_eq!(intact.fail(check).check(), Err(reason), "evidence order");
    }
    let bytes = b"output bytes".to_vec();
    let observed = vec![("out.json".to_owned(), bytes.clone(), digest_b3(&bytes))];
    let declared = vec!["out.json".to_owned()];
    assert!(
        verify_restored_task_result("clippy", intact, &declared, &observed).is_ok(),
        "intact restore with verified outputs verifies"
    );
    assert_eq!(
        verify_restored_task_result("clippy", intact, &declared, &[]),
        Err(MissReason::TASK_RESULT_INCOMPLETE),
        "missing output is incomplete"
    );
    let tampered = vec![("out.json".to_owned(), b"other".to_vec(), digest_b3(&bytes))];
    assert_eq!(
        verify_restored_task_result("clippy", intact, &declared, &tampered),
        Err(MissReason::TASK_RESULT_INCOMPLETE),
        "mismatched output is incomplete"
    );
    assert_eq!(
        verify_restored_task_result(
            "clippy",
            intact.fail(RestoreCheck::EntryPresent),
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
        assert_eq!(fallback_for_error(error), *reason, "map {error}");
    }
    assert_eq!(cases.len(), 22, "every variant mapped");
    let fallback = ReuseFallback::execute_with(MissReason::CACHE_UNAVAILABLE);
    assert_eq!(fallback.reason(), MissReason::CACHE_UNAVAILABLE);
    assert!(fallback.proceeds_to_execute(), "miss never fails");
}
