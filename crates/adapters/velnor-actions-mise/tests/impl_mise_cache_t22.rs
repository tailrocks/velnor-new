//! T22 provider-cache negatives: never-archive exclusions, corruption,
//! poisoned links, wrong platform/tool/lock, and cold recovery.
use std::path::PathBuf;

use velnor_actions_contract::cachekey::MISS_REASONS;
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::cache_sources as sources;
use velnor_actions_mise::restore::{
    MissReason, RestoreEvidence, ReuseFallback, SaveInputs, fallback_for_error, save_decision,
    verify_restored_task_result,
};
use velnor_actions_mise::restore_evidence::{
    RestoreObservation, output_bytes_complete, verify_provider_restore,
};
use velnor_actions_mise::{MiseError, read_artifact_bytes, verify_artifact_digest};

fn scratch_dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("velnor-mise-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) | Err(_) => {}
    }
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

/// Fully observed provider restore: real path, bytes, matching digests.
fn observed_provider_restore() -> RestoreObservation {
    let bytes = b"provider bytes".to_vec();
    RestoreObservation {
        entry_path: "tofu-cache/root-0123456789ab/registry.opentofu.org".to_owned(),
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

#[test]
fn never_archive_markers_reject_state_plans_and_credentials() {
    assert_eq!(sources::NEVER_ARCHIVE_MARKERS.len(), 3);
    for marker in [".tfstate", ".tfplan", "credentials"] {
        assert!(
            sources::NEVER_ARCHIVE_MARKERS.contains(&marker),
            "missing marker {marker}"
        );
        assert!(
            sources::is_never_archive_path(&format!("/cache/x{marker}")),
            "{marker} must flag"
        );
    }
    for clean in [
        "/cache/registry/cache/serde-1.0.228.crate",
        "/cache/.crates.toml",
        "/cache/bin/cargo-nextest",
        "/cache/git/db/objects/pack",
        // Deliberate boundary (B12): secrets/token substrings do NOT
        // flag — only the three markers above exclude.
        "/cache/config/secret.token",
        "/cache/config/secrets.env",
        "/cache/id_token.txt",
    ] {
        assert!(
            !sources::is_never_archive_path(clean),
            "{clean} must stay archivable"
        );
    }
}

#[test]
fn sources_subset_validation_enforces_the_never_archive_list() {
    let home = "${{ runner.temp }}/velnor/cargo";
    for bad in [
        format!("{home}/registry/cache/state.tfstate"),
        format!("{home}/registry/cache/state.tfstate.backup"),
        format!("{home}/registry/cache/plan.tfplan"),
        format!("{home}/registry/cache/plan.tfplan.json"),
        format!("{home}/registry/cache/credentials.toml"),
        format!("{home}/bin/evil.tfstate"),
    ] {
        assert!(
            sources::validate_sources_subset(std::slice::from_ref(&bad), home).is_err(),
            "must reject {bad}"
        );
    }
    let good = sources::sources_cache_paths(home).expect("subset paths");
    assert!(
        sources::validate_sources_subset(&good, home).is_ok(),
        "the sufficient subset stays archivable"
    );
}

#[test]
fn provider_corruption_discards_and_refetches_through_the_model_checks() {
    let mut tampered = observed_provider_restore();
    tampered.entry_bytes = b"tampered bytes".to_vec();
    assert_eq!(verify_provider_restore(&tampered), Err("cache_corrupt"));
    let evidence = RestoreEvidence::verify(&tampered);
    assert_eq!(evidence.check(), Err(MissReason::CACHE_CORRUPT));
    let decision = sources::fetch_decision(false, "cache_corrupt").expect("fetch");
    assert_eq!(
        decision,
        sources::FetchDecision::ExplicitFetch {
            miss_reason: "cache_corrupt",
        }
    );
    let fallback = ReuseFallback::execute_with(MissReason::CACHE_CORRUPT);
    assert!(fallback.proceeds_to_execute(), "a miss never fails a task");
}

#[test]
fn provider_restore_checks_run_in_order_with_precise_reasons() {
    let hit = observed_provider_restore();
    assert_eq!(verify_provider_restore(&hit), Ok(()));
    let mut missing = observed_provider_restore();
    missing.entry_path.clear();
    missing.entry_bytes = b"tampered bytes".to_vec();
    assert_eq!(
        verify_provider_restore(&missing),
        Err("no_entry"),
        "presence wins over digest"
    );
    let mut corrupt = observed_provider_restore();
    corrupt.entry_bytes = b"tampered bytes".to_vec();
    assert_eq!(verify_provider_restore(&corrupt), Err("cache_corrupt"));
    let mut malformed = observed_provider_restore();
    malformed.expected_digest = "not-a-digest".to_owned();
    assert_eq!(
        verify_provider_restore(&malformed),
        Err("cache_corrupt"),
        "malformed digests never match"
    );
    let mut wrong_arch = observed_provider_restore();
    wrong_arch.observed_compat = digest_b3(b"other-platform");
    assert_eq!(
        verify_provider_restore(&wrong_arch),
        Err("compatibility_mismatch")
    );
    let mut wrong_owner = observed_provider_restore();
    wrong_owner.observed_owner = "pr".to_owned();
    assert_eq!(
        verify_provider_restore(&wrong_owner),
        Err("trust_scope_mismatch")
    );
    let mut wrong_inputs = observed_provider_restore();
    wrong_inputs.observed_inputs = digest_b3(b"other-inputs");
    assert_eq!(
        verify_provider_restore(&wrong_inputs),
        Err("input_digest_mismatch")
    );
}

#[test]
fn zero_byte_provider_entries_fail_the_p04_rule() {
    assert!(!output_bytes_complete(&[]));
    assert!(output_bytes_complete(b"x"));
    let mut empty = observed_provider_restore();
    empty.entry_bytes = Vec::new();
    assert_eq!(verify_provider_restore(&empty), Err("cache_corrupt"));
}

#[test]
fn provider_restore_errors_stay_inside_the_closed_thirteen() {
    assert_eq!(MissReason::ALL.len(), 13);
    let mut corrupt = observed_provider_restore();
    corrupt.entry_bytes = b"tampered bytes".to_vec();
    let mut wrong_arch = observed_provider_restore();
    wrong_arch.observed_compat = digest_b3(b"other-platform");
    for obs in [observed_provider_restore(), corrupt, wrong_arch] {
        if let Err(reason) = verify_provider_restore(&obs) {
            assert!(
                MISS_REASONS.contains(&reason),
                "{reason} must be a closed miss reason"
            );
        }
    }
}

#[test]
fn restored_results_verify_evidence_before_outputs() {
    let hit = observed_provider_restore();
    let evidence = RestoreEvidence::verify(&hit);
    let declared = vec!["report.json".to_owned()];
    let bytes = b"report".to_vec();
    let observed = vec![("report.json".to_owned(), bytes.clone(), digest_b3(&bytes))];
    assert!(verify_restored_task_result("clippy", evidence, &declared, &observed).is_ok());
    let mut corrupt = observed_provider_restore();
    corrupt.entry_bytes = b"tampered bytes".to_vec();
    let evidence = RestoreEvidence::verify(&corrupt);
    assert_eq!(
        verify_restored_task_result("clippy", evidence, &declared, &observed),
        Err(MissReason::CACHE_CORRUPT)
    );
}

#[test]
#[cfg(unix)]
fn symlinked_cache_entry_with_tampered_bytes_fails_closed() -> Result<(), String> {
    let dir = scratch_dir("t22-link")?;
    let outside = dir.join("outside.bin");
    std::fs::write(&outside, b"tampered bytes").map_err(|err| err.to_string())?;
    let link = dir.join("entry.bin");
    std::os::unix::fs::symlink(&outside, &link).map_err(|err| err.to_string())?;
    let bytes = read_artifact_bytes(&link).map_err(|err| err.to_string())?;
    let expected = digest_b3(b"provider bytes");
    assert!(verify_artifact_digest(&bytes, &expected).is_err());
    let mut obs = observed_provider_restore();
    obs.entry_path = link.to_string_lossy().into_owned();
    obs.entry_bytes = bytes;
    obs.expected_digest = expected;
    assert_eq!(
        RestoreEvidence::verify(&obs).check(),
        Err(MissReason::CACHE_CORRUPT)
    );
    let fallback = ReuseFallback::execute_with(MissReason::CACHE_CORRUPT);
    assert!(fallback.proceeds_to_execute(), "a miss never fails a task");
    std::fs::remove_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_loop_maps_to_unavailable_and_still_executes() -> Result<(), String> {
    let dir = scratch_dir("t22-loop")?;
    let link = dir.join("loop.bin");
    std::os::unix::fs::symlink(&link, &link).map_err(|err| err.to_string())?;
    let err = read_artifact_bytes(&link).expect_err("loops never read");
    assert!(
        matches!(err, MiseError::ArtifactUnreadable { .. }),
        "unexpected {err:?}"
    );
    let reason = fallback_for_error(&err).map_err(|err| err.to_string())?;
    assert_eq!(reason, MissReason::CACHE_UNAVAILABLE);
    assert!(ReuseFallback::execute_with(reason).proceeds_to_execute());
    std::fs::remove_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(())
}

#[test]
fn wrong_tool_version_cools_to_tool_missing_and_executes() {
    let err = MiseError::InvalidToolVersion {
        tool: "opentofu".to_owned(),
        version: "1.13".to_owned(),
    };
    let reason = fallback_for_error(&err).expect("maps, never propagates");
    assert_eq!(reason, MissReason::TOOL_MISSING);
    assert!(ReuseFallback::execute_with(reason).proceeds_to_execute());
}

#[test]
fn fetch_decision_keeps_its_closed_reason_set() {
    assert_eq!(
        sources::fetch_decision(true, "no_entry").expect("skip"),
        sources::FetchDecision::OfflineSkip
    );
    for reason in [
        "no_entry",
        "source_missing",
        "cache_unavailable",
        "cache_corrupt",
    ] {
        assert_eq!(
            sources::fetch_decision(false, reason).expect("fetch"),
            sources::FetchDecision::ExplicitFetch {
                miss_reason: reason
            },
            "{reason} must fetch"
        );
    }
    for reason in ["bogus", "compatibility_mismatch", "task_result_incomplete"] {
        assert!(
            sources::fetch_decision(false, reason).is_err(),
            "{reason} must stay outside the fetch set"
        );
    }
}

#[test]
fn save_decision_keeps_single_writer_push_gated_saves() {
    fn save<'a>(
        layer_trust: &'a str,
        event: &'a str,
        passed: bool,
        active_writer: bool,
    ) -> SaveInputs<'a> {
        SaveInputs {
            layer_trust,
            event,
            passed,
            unavailable: false,
            active_writer,
        }
    }
    assert!(save_decision(&save("trusted", "push", true, false)).is_ok());
    assert!(save_decision(&save("pr", "push", true, false)).is_ok());
    assert_eq!(
        save_decision(&save("trusted", "push", true, true)),
        Err(MissReason::CACHE_WRITE_DISABLED),
        "one writer per key"
    );
    for event in ["pull_request", "fork", "merge_group", "release", "local"] {
        assert_eq!(
            save_decision(&save("trusted", event, true, false)),
            Err(MissReason::CACHE_WRITE_DISABLED),
            "{event} never saves"
        );
    }
    assert_eq!(
        save_decision(&save("trusted", "push", false, false)),
        Err(MissReason::CACHE_WRITE_DISABLED),
        "failed runs never save"
    );
    assert_eq!(
        save_decision(&save("unknown", "push", true, false)),
        Err(MissReason::CACHE_WRITE_DISABLED),
        "unknown trust denies closed"
    );
    assert_eq!(
        save_decision(&SaveInputs {
            layer_trust: "trusted",
            event: "push",
            passed: true,
            unavailable: true,
            active_writer: false,
        }),
        Err(MissReason::CACHE_UNAVAILABLE)
    );
}
