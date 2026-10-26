// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C07: PR runs never publish, even with a complete candidate.
pub(crate) fn check_pr_runs_never_publish() -> Result<(), Box<dyn Error>> {
    let mode = CacheMode::explicit_read();
    if !mode.is_explicit_read() {
        return Err(invalid("PR consumer lost its explicit read mode"));
    }
    if mode.grants_write() {
        return Err(invalid("PR read mode grants cache writes"));
    }
    let denied = lifecycle_save(
        false,
        Some("pr-event-requires-trusted-authority"),
        true,
        false,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("PR save escaped its denial"));
    }
    let denied_existing = lifecycle_save(
        false,
        Some("pr-event-requires-trusted-authority"),
        true,
        true,
        SaveHalt::Running,
    );
    if decide_save(&denied_existing)? != SaveOutcome::Denied {
        return Err(invalid("PR save published over an existing entry"));
    }
    let admitted = lifecycle_save(true, None, true, false, SaveHalt::Running);
    if decide_save(&admitted)? != SaveOutcome::Attempted {
        return Err(invalid("admission lost its trusted save attempt"));
    }
    Ok(())
}

/// C07: application-only reruns reuse the trusted baseline.
pub(crate) fn check_app_rerun_reuses_trusted_baseline() -> Result<(), Box<dyn Error>> {
    let base_coverage = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&base_coverage, &app_only)?;
    let dep_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
    require_changed_coverage(&base_coverage, &dep_bump)?;
    let base = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let job = lifecycle_job("pull_request", "refs/pull/7/merge", 71, 1, 711, "reader")?;
    let request = lifecycle_discovery(
        job,
        base.clone(),
        vec!["refs/heads/main", "refs/pull/7/merge"],
        "refs/pull/7/merge",
        vec![lifecycle_entry(
            &base,
            "refs/heads/main",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("app-only rerun missed the trusted baseline"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(invalid("single-ref baseline reported a shadow"));
    }
    Ok(())
}

/// C07: new PR dependencies rebuild until trusted main preparation exists,
/// then the merged preparation is reused exactly once.
pub(crate) fn check_dependency_change_rebuilds_until_trusted() -> Result<(), Box<dyn Error>> {
    let old = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let changed_seed = b"gc1-qualification-lifecycle-changed";
    let changed = lifecycle_generation(changed_seed)?;
    if old.cache_key() == changed.cache_key() {
        return Err(invalid("dependency change kept the old cache key"));
    }
    let pr_job = lifecycle_job("pull_request", "refs/pull/7/merge", 71, 1, 712, "reader")?;
    let missing = lifecycle_discovery(
        pr_job,
        changed.clone(),
        vec!["refs/heads/main", "refs/pull/7/merge"],
        "refs/pull/7/merge",
        vec![lifecycle_entry(
            &old,
            "refs/heads/main",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("changed coverage hit the stale baseline"));
    }
    let trusted_job = lifecycle_job("push", "refs/heads/main", 72, 1, 713, "writer")?;
    let frozen = lifecycle_frozen()?;
    lifecycle_proof(&frozen, changed_seed, trusted_job.clone())?;
    let prepared = lifecycle_discovery(
        trusted_job,
        changed.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &changed,
            "refs/heads/main",
            72,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let prepared_record =
        decide_discovery(&prepared).map_err(|error| invalid(&error.to_string()))?;
    if prepared_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("trusted preparation missed its exact hit"));
    }
    let repeat = lifecycle_save(true, None, true, true, SaveHalt::Running);
    if decide_save(&repeat)? != SaveOutcome::SkippedExisting {
        return Err(invalid("prepared coverage was prepared twice"));
    }
    Ok(())
}

/// C07: PR compiler output is never promoted through a shared cache prefix.
pub(crate) fn check_pr_output_never_promotes_prefix() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-baseline")?;
    let main_job = lifecycle_job("push", "refs/heads/main", 73, 1, 714, "reader")?;
    let request = lifecycle_discovery(
        main_job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/pull/7/merge",
            71,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("main request consumed a PR-scoped entry"));
    }
    Ok(())
}

/// C07: preview and stable builds consume their correct prepared variants.
pub(crate) fn check_preview_stable_consume_prepared() -> Result<(), Box<dyn Error>> {
    let preview = lifecycle_generation(b"gc1-qualification-lifecycle-preview")?;
    let stable = lifecycle_generation(b"gc1-qualification-lifecycle-stable")?;
    if preview.cache_key() == stable.cache_key() {
        return Err(invalid("preview and stable share one cache key"));
    }
    for (generation, job_id) in [(&preview, 715_u64), (&stable, 716_u64)] {
        let job = lifecycle_job("push", "refs/heads/main", 74, 1, job_id, "reader")?;
        let request = lifecycle_discovery(
            job,
            generation.clone(),
            vec!["refs/heads/main"],
            "refs/heads/main",
            vec![lifecycle_entry(
                generation,
                "refs/heads/main",
                job_id,
                EntryProvenance::Trusted,
            )],
            true,
        );
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::ExactHit {
            return Err(invalid("prepared profile missed its exact hit"));
        }
    }
    let crossed = RestoreObservation {
        intended_key: preview.cache_key(),
        matched_key: stable.cache_key(),
        restored_compatibility: preview.compatibility_digest.clone(),
        expected_compatibility: preview.compatibility_digest.clone(),
        restored_coverage: stable.coverage_digest.clone(),
        expected_coverage: preview.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-lifecycle-manifest"),
    };
    must_fail(
        &verify_restore(&crossed),
        "stable entry admitted for a preview request",
    )?;
    Ok(())
}

/// C08: fork PRs stay under restricted PR authority.
pub(crate) fn check_fork_event_restricted() -> Result<(), Box<dyn Error>> {
    if trust_for_event(WorkflowEvent::Fork) != Trust::Pr {
        return Err(invalid("fork escaped PR authority"));
    }
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 811, "reader")?;
    if job.event.is_empty() || job.ref_name.is_empty() {
        return Err(invalid("fork job lost its event binding"));
    }
    Ok(())
}

/// C08: fork consumers are read-only; origin credentials and private cache
/// contents stay outside unauthorized exposure.
pub(crate) fn check_fork_cache_read_only() -> Result<(), Box<dyn Error>> {
    let mode = CacheMode::explicit_read();
    if mode.grants_write() || mode.grants_unconditional_write() {
        return Err(invalid("fork read mode grants cache writes"));
    }
    let denied = lifecycle_save(
        false,
        Some("fork-restricted-capability"),
        true,
        false,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("fork save escaped its denial"));
    }
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-fork")?;
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 812, "reader")?;
    let request = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/pull/8/merge",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/private",
            81,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("fork request consumed a private entry"));
    }
    Ok(())
}

/// C08: private-source denial is explicit, and authorized fork work recovers
/// cold with a real nonzero baseline instead of minted credentials.
pub(crate) fn check_private_source_denied_cold_recovery() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-fork")?;
    let job = lifecycle_job("pull_request", "refs/pull/8/merge", 81, 1, 813, "reader")?;
    for provenance in [EntryProvenance::Untrusted, EntryProvenance::Ambiguous] {
        let request = lifecycle_discovery(
            job.clone(),
            generation.clone(),
            vec!["refs/heads/main", "refs/pull/8/merge"],
            "refs/pull/8/merge",
            vec![
                lifecycle_entry(&generation, "refs/heads/main", 81, provenance),
                lifecycle_entry(&generation, "refs/heads/main", 82, EntryProvenance::Trusted),
            ],
            true,
        );
        let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
        if record.decision != DiscoveryDecision::RefusedProvenance {
            return Err(invalid("private entry escaped provenance refusal"));
        }
    }
    let missing = lifecycle_discovery(
        job,
        generation,
        vec!["refs/heads/main", "refs/pull/8/merge"],
        "refs/pull/8/merge",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("missing fork coverage skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("fork cold recovery measured zero work"));
    }
    Ok(())
}
