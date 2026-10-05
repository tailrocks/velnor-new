//! Restore verification and miss-reason fallback cases.
use velnor_actions_contract::cachekey::MISS_REASONS;
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::MiseError;
use velnor_actions_mise::restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, fallback_for_error,
    verify_restored_task_result,
};
use velnor_actions_mise::restore_evidence::RestoreObservation;
use velnor_actions_mise::restore_evidence::verify_provider_restore;

/// Fully observed restore: real path, bytes, and matching digests.
fn observed_restore() -> RestoreObservation {
    let bytes = b"entry bytes".to_vec();
    RestoreObservation {
        entry_path: "task-artifacts/v2/clippy/entry".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"compat"),
        observed_compat: digest_b3(b"compat"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"inputs"),
        observed_inputs: digest_b3(b"inputs"),
    }
}

/// REUSE-5: evidence checks run present/digest/compat/owner/inputs, then
/// every declared output must be present and verified. Evidence builds
/// only from observed restores via `verify` (no `intact()` shortcut, no
/// bare-bool constructor): a real observed restore passes, and each
/// unverified input fails with its precise reason.
#[test]
fn restore_verification_orders_evidence_then_outputs() {
    let verified = RestoreEvidence::verify(&observed_restore());
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
    let check = |label: &str, obs: &RestoreObservation, reason: MissReason| {
        assert_eq!(RestoreEvidence::verify(obs).check(), Err(reason), "{label}");
    };
    let mut obs = observed_restore();
    obs.entry_path.clear();
    check("missing entry", &obs, MissReason::NO_ENTRY);
    let mut obs = observed_restore();
    obs.entry_bytes = b"forged".to_vec();
    check("tampered bytes", &obs, MissReason::CACHE_CORRUPT);
    let mut obs = observed_restore();
    obs.observed_compat = digest_b3(b"other");
    check("compat drift", &obs, MissReason::COMPATIBILITY_MISMATCH);
    let mut obs = observed_restore();
    obs.observed_owner = "pr".to_owned();
    check("owner drift", &obs, MissReason::TRUST_SCOPE_MISMATCH);
    let mut obs = observed_restore();
    obs.observed_inputs = digest_b3(b"other");
    check("input drift", &obs, MissReason::INPUT_DIGEST_MISMATCH);
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

/// Fully observed provider restore: lock-bound bytes, matching dims.
fn provider_observation() -> RestoreObservation {
    let bytes = b"provider bytes".to_vec();
    RestoreObservation {
        entry_path: "tofu-cache/b3-0000000000000000000000000000000000000000000000000000000000000000/registry.opentofu.org/hashicorp/null".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"provider-compat"),
        observed_compat: digest_b3(b"provider-compat"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"lock-inputs"),
        observed_inputs: digest_b3(b"lock-inputs"),
    }
}

/// T21: provider-cache restores classify through the same model
/// 5-check chain as task results; a hit never disables verification.
#[test]
fn provider_restore_classifies_through_the_model_five_check_chain() {
    let ok = provider_observation();
    assert_eq!(verify_provider_restore(&ok), Ok(()));
    assert_eq!(
        verify_provider_restore(&ok),
        Ok(()),
        "verification is pure: a hit re-verifies"
    );
    let mut obs = provider_observation();
    obs.entry_path.clear();
    assert_eq!(verify_provider_restore(&obs), Err("no_entry"));
    let mut obs = provider_observation();
    obs.entry_bytes = b"forged".to_vec();
    assert_eq!(
        verify_provider_restore(&obs),
        Err("cache_corrupt"),
        "corrupt content discards, never verifies"
    );
    let mut obs = provider_observation();
    obs.observed_compat = digest_b3(b"other");
    assert_eq!(verify_provider_restore(&obs), Err("compatibility_mismatch"));
    let mut obs = provider_observation();
    obs.observed_owner = "pr".to_owned();
    assert_eq!(verify_provider_restore(&obs), Err("trust_scope_mismatch"));
    let mut obs = provider_observation();
    obs.observed_inputs = digest_b3(b"other");
    assert_eq!(verify_provider_restore(&obs), Err("input_digest_mismatch"));
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
    for message in [
        "cancelled",
        "timeout_after_secs:30",
        "timeout_after_secs:0",
        "cancelled;cleanup_failed:kill_group:permission denied",
        "timeout_after_absolute_deadline;cleanup_failed:reap_child:still running",
    ] {
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
