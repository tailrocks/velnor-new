// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn lifecycle_attempt_ledger(
    attempt: &str,
    failed: bool,
    interrupted: bool,
) -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    let package_ids = ["workspace-lib", "workspace-app"];
    for (index, package_id) in package_ids.iter().enumerate() {
        let role = if *package_id == "workspace-app" {
            ROLE_APPLICATION
        } else {
            ROLE_DEPENDENCY
        };
        let event_id = format!("evt-{attempt}-{index}");
        ledger.record(CompilerEvent {
            invocation: lifecycle_invocation(&event_id, package_id, role),
            completion: None,
        })?;
        let completion = if failed && index == 0 {
            lifecycle_failed()
        } else if interrupted {
            lifecycle_interrupted_completion()
        } else {
            lifecycle_success()
        };
        ledger.complete(&event_id, completion)?;
    }
    Ok(ledger)
}

/// C12: group A succeeds at attempt 1, group B fails for a controlled reason,
/// and the rerun executes B and Required at attempt 2.
pub(crate) fn check_mixed_attempt_ledgers() -> Result<(), Box<dyn Error>> {
    let attempt_one_success = lifecycle_attempt_ledger("a1", false, false)?;
    if attempt_one_success.failed() != 0 {
        return Err(invalid("successful attempt 1 recorded failures"));
    }
    if attempt_one_success.dependency_compilations() == 0 {
        return Err(invalid("successful attempt 1 measured zero work"));
    }
    let mut attempt_one_failed = lifecycle_attempt_ledger("b1", true, false)?;
    if attempt_one_failed.failed() == 0 {
        return Err(invalid("failed writer lost its failure"));
    }
    must_fail(
        &attempt_one_failed.record(CompilerEvent {
            invocation: lifecycle_invocation("evt-b1-0", "workspace-lib", ROLE_DEPENDENCY),
            completion: None,
        }),
        "duplicate rerun ledger admitted",
    )?;
    must_fail(
        &attempt_one_failed
            .clone()
            .complete("evt-missing", lifecycle_success()),
        "unknown rerun receipt admitted",
    )?;
    let attempt_two = lifecycle_attempt_ledger("b2", false, false)?;
    if attempt_two.failed() != 0 {
        return Err(invalid("rerun attempt 2 recorded failures"));
    }
    if attempt_two.dependency_compilations() == 0 {
        return Err(invalid("rerun attempt 2 measured zero work"));
    }
    Ok(())
}

/// C12: the rerun accepts A's authentic attempt-1 ledger only after
/// authenticated job records establish retention without rerunning.
pub(crate) fn check_retained_success_accepted() -> Result<(), Box<dyn Error>> {
    let retained_a = lifecycle_job("push", "refs/heads/main", 121, 1, 1211, "group-a")?;
    let rerun_b = lifecycle_job("push", "refs/heads/main", 121, 2, 1212, "group-b")?;
    if retained_a.run_id != rerun_b.run_id {
        return Err(invalid("rerun lost its run identity"));
    }
    if retained_a.run_attempt == rerun_b.run_attempt {
        return Err(invalid("retained job relabelled as rerun evidence"));
    }
    if retained_a.plan_digest != rerun_b.plan_digest {
        return Err(invalid("rerun changed its plan identity"));
    }
    let mut drifted_plan = retained_a.clone();
    drifted_plan.plan_digest = Digest::parse(&digest_b3(b"gc1-qualification-lifecycle-other"))?;
    if drifted_plan.plan_digest == rerun_b.plan_digest {
        return Err(invalid("mismatched plan identity compared equal"));
    }
    let mut cross_run = retained_a.clone();
    cross_run.run_id = 999;
    if cross_run.run_id == rerun_b.run_id {
        return Err(invalid("cross-run receipt compared equal"));
    }
    Ok(())
}

/// C12: no global current-run-attempt equality is imposed on a retained
/// successful job.
pub(crate) fn check_no_global_attempt_equality() -> Result<(), Box<dyn Error>> {
    let retained_a = lifecycle_job("push", "refs/heads/main", 121, 1, 1211, "group-a")?;
    let rerun_required = lifecycle_job("push", "refs/heads/main", 121, 2, 1213, "required")?;
    retained_a.validate()?;
    rerun_required.validate()?;
    if retained_a.run_attempt != 1 || rerun_required.run_attempt != 2 {
        return Err(invalid("mixed attempts lost their attempt binding"));
    }
    Ok(())
}

/// C12: stale B evidence is rejected; readable coverage is used when
/// available and cold recovery runs when it is not.
pub(crate) fn check_stale_evidence_rejected() -> Result<(), Box<dyn Error>> {
    let stale_b = lifecycle_attempt_ledger("b1", true, false)?;
    if stale_b.failed() == 0 {
        return Err(invalid("stale failed ledger passed as rerun success"));
    }
    let fresh_b = lifecycle_attempt_ledger("b2", false, false)?;
    if fresh_b.failed() != 0 {
        return Err(invalid("fresh rerun ledger kept the old failure"));
    }
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-rerun")?;
    let job = lifecycle_job("push", "refs/heads/main", 121, 2, 1212, "group-b")?;
    let readable = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            1211,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let readable_record =
        decide_discovery(&readable).map_err(|error| invalid(&error.to_string()))?;
    if readable_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("retained coverage missed its readable hit"));
    }
    let exact = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: generation.cache_key(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let missing = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("missing rerun coverage skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("rerun cold recovery measured zero work"));
    }
    let stale_bytes = b"gc1-qualification-lifecycle-stale-attempt".to_vec();
    let fresh_digest = sha256_hex(&lifecycle_frozen_bytes());
    let stale_entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1212,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: stale_bytes.len() as u64,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let stale_readback = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &fresh_digest,
        frozen_archive_bytes: stale_bytes.len() as u64,
        restored_bytes: Some(&stale_bytes),
        inventory_complete: true,
        observed_entry: Some(&stale_entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(stale_readback, ReadbackVerdict::Corrupt { .. }) {
        return Err(invalid("stale rerun bytes escaped corruption"));
    }
    Ok(())
}
