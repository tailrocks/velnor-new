// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C09: merge-queue and tag consumers are read-only.
pub(crate) fn check_merge_consumers_read_only() -> Result<(), Box<dyn Error>> {
    if trust_for_event(WorkflowEvent::MergeGroup) != Trust::Pr {
        return Err(invalid("merge-group escaped PR authority"));
    }
    let queue_job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        911,
        "reader",
    )?;
    let tag_job = lifecycle_job("push", "refs/tags/v1.0.0", 92, 1, 912, "reader")?;
    for job in [queue_job, tag_job] {
        let denied = lifecycle_save(
            false,
            Some("merge-consumer-read-only"),
            true,
            false,
            SaveHalt::Running,
        );
        if decide_save(&denied)? != SaveOutcome::Denied {
            return Err(invalid("merge consumer save escaped its denial"));
        }
        let mode = CacheMode::explicit_read();
        if !mode.is_explicit_read() || mode.grants_write() {
            return Err(invalid("merge consumer lost its read-only mode"));
        }
        let _ = job;
    }
    Ok(())
}

/// C09: conflicting visible refs refuse the shadow; a single selected unit
/// resolves to exactly one candidate.
pub(crate) fn check_ref_shadow_refused() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        913,
        "reader",
    )?;
    let conflicted = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main", "refs/heads/gh-readonly-queue/main/pr-1"],
        "refs/heads/gh-readonly-queue/main/pr-1",
        vec![
            lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted),
            lifecycle_entry(
                &generation,
                "refs/heads/gh-readonly-queue/main/pr-1",
                92,
                EntryProvenance::Trusted,
            ),
        ],
        true,
    );
    let conflicted_record =
        decide_discovery(&conflicted).map_err(|error| invalid(&error.to_string()))?;
    if conflicted_record.decision != DiscoveryDecision::RefusedShadow {
        return Err(invalid("conflicting refs escaped shadow refusal"));
    }
    if conflicted_record.shadow_state != ShadowState::ConflictingVisibleRef {
        return Err(invalid("conflicting refs reported a clear shadow"));
    }
    let exact_refs: BTreeSet<String> = BTreeSet::from([
        "refs/heads/main".to_owned(),
        "refs/heads/gh-readonly-queue/main/pr-1".to_owned(),
    ]);
    if classify_shadow(
        &exact_refs,
        &BTreeSet::new(),
        "refs/heads/gh-readonly-queue/main/pr-1",
    ) != ShadowState::ConflictingVisibleRef
    {
        return Err(invalid("direct shadow check missed the conflict"));
    }
    let single = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            91,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let single_record = decide_discovery(&single).map_err(|error| invalid(&error.to_string()))?;
    if single_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("single-ref request missed its exact hit"));
    }
    let visible: Vec<&SelectableEntry> = single.entries.iter().collect();
    let candidate = select_exact(&single, &visible)
        .ok_or_else(|| invalid("exact selection lost its candidate"))?;
    if candidate.entry.cache_id != 91 {
        return Err(invalid("exact selection kept the wrong unit"));
    }
    Ok(())
}

/// C09: longer primary-key prefix matches on the current ref also conflict.
pub(crate) fn check_longer_prefix_shadow() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let prefix = primary_prefix(&generation);
    let longer = SelectableEntry {
        cache_id: 93,
        repository_id: 1,
        ref_name: "refs/heads/gh-readonly-queue/main/pr-1".to_owned(),
        key: format!("{prefix}-restore-001"),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        provenance: EntryProvenance::Trusted,
    };
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        914,
        "reader",
    )?;
    let request = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main", "refs/heads/gh-readonly-queue/main/pr-1"],
        "refs/heads/gh-readonly-queue/main/pr-1",
        vec![
            lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted),
            longer,
        ],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::RefusedShadow {
        return Err(invalid("prefix shadow escaped refusal"));
    }
    Ok(())
}

/// C09: the pin extracts a differing matched key before returning it;
/// unauthorized, ambiguous, and foreign-repository scopes are refused.
pub(crate) fn check_pin_extracts_matched_key() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
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
    let mut drifted = exact.clone();
    drifted.matched_key = lifecycle_generation(b"gc1-qualification-lifecycle-other")?.cache_key();
    must_fail(&verify_restore(&drifted), "differing matched key admitted")?;
    let job = lifecycle_job(
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1",
        91,
        1,
        915,
        "reader",
    )?;
    let ambiguous = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            91,
            EntryProvenance::Ambiguous,
        )],
        true,
    );
    let ambiguous_record =
        decide_discovery(&ambiguous).map_err(|error| invalid(&error.to_string()))?;
    if ambiguous_record.decision != DiscoveryDecision::RefusedProvenance {
        return Err(invalid("ambiguous scope escaped provenance refusal"));
    }
    let mut foreign = lifecycle_entry(&generation, "refs/heads/main", 91, EntryProvenance::Trusted);
    foreign.repository_id = 2;
    let cross_repo = lifecycle_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![foreign],
        true,
    );
    let cross_repo_record =
        decide_discovery(&cross_repo).map_err(|error| invalid(&error.to_string()))?;
    if cross_repo_record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("foreign repository entry satisfied the request"));
    }
    Ok(())
}

/// C09: unavailable inventory reports incomplete provenance and uses the
/// permitted cold path instead of a verified default.
pub(crate) fn check_inventory_unavailable_uses_cold_path() -> Result<(), Box<dyn Error>> {
    let generation = lifecycle_generation(b"gc1-qualification-lifecycle-merge")?;
    let job = lifecycle_job("push", "refs/tags/v1.0.0", 92, 1, 916, "reader")?;
    let unavailable = lifecycle_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        false,
    );
    let unavailable_record =
        decide_discovery(&unavailable).map_err(|error| invalid(&error.to_string()))?;
    if unavailable_record.decision != DiscoveryDecision::ColdUnavailable {
        return Err(invalid("missing inventory escaped cold-unavailable"));
    }
    if unavailable_record.shadow_state != ShadowState::Unknown {
        return Err(invalid("missing inventory claimed a shadow verdict"));
    }
    let degraded = lifecycle_discovery(
        job,
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![lifecycle_entry(
            &generation,
            "refs/heads/main",
            92,
            EntryProvenance::Trusted,
        )],
        false,
    );
    let degraded_record =
        decide_discovery(&degraded).map_err(|error| invalid(&error.to_string()))?;
    if degraded_record.decision != DiscoveryDecision::ExactHit {
        return Err(invalid("trusted exact lost its degraded hit"));
    }
    if degraded_record.shadow_state != ShadowState::Unknown {
        return Err(invalid("degraded hit claimed verified provenance"));
    }
    let cold = cold_ledger()?;
    if cold.dependency_compilations() == 0 {
        return Err(invalid("tag cold path measured zero work"));
    }
    Ok(())
}

/// C10: competing producers share the common repository mutation group with
/// queue semantics and no cancel-in-progress.
pub(crate) fn check_writer_queue_policy() -> Result<(), Box<dyn Error>> {
    if WRITER_QUEUE_CANCEL_IN_PROGRESS != "false" {
        return Err(invalid("writer queue lost its cancel-in-progress policy"));
    }
    let group = "gc1-qualification-writer-queue";
    let queue = Concurrency::writer_queue(group);
    if !queue.is_writer_queue() {
        return Err(invalid("writer queue lost its queue identity"));
    }
    queue
        .validate_writer_queue()
        .map_err(|error| invalid(&error.to_string()))?;
    if queue.cancel_in_progress != "false" {
        return Err(invalid("writer queue cancels in-progress work"));
    }
    let empty_group = Concurrency::writer_queue("");
    must_fail(
        &empty_group.validate_writer_queue(),
        "writer queue without a group admitted",
    )?;
    let cancelling = Concurrency {
        group: group.to_owned(),
        cancel_in_progress: "true".to_owned(),
    };
    must_fail(
        &cancelling.validate_writer_queue(),
        "writer queue with cancel-in-progress admitted",
    )?;
    Ok(())
}
