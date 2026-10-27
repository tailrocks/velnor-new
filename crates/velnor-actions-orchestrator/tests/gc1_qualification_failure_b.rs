// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C14: removing only the local transport copy before readback classifies the
/// absent publication as unavailable, never as verified or corrupt.
pub(crate) fn check_denied_absent_never_claims_entry() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-denied")?;
    let entry = ObservedEntry {
        repository_id: 1,
        cache_id: 1411,
        ref_name: "refs/heads/main".to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: size,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
    };
    let present = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: Some(&bytes),
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(present, ReadbackVerdict::Verified { .. }) {
        return Err(invalid("exact readback missed its verification"));
    }
    let transport_removed = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: Some(&entry),
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(transport_removed, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("removed transport copy claimed a verdict"));
    }
    let no_entry = decide_readback(&ReadbackInputs {
        frozen_archive_digest: &digest,
        frozen_archive_bytes: size,
        restored_bytes: None,
        inventory_complete: true,
        observed_entry: None,
        expected_key: &generation.cache_key(),
        expected_ref: "refs/heads/main",
        expected_version: "v1",
    })?;
    if !matches!(no_entry, ReadbackVerdict::Unavailable { .. }) {
        return Err(invalid("absent publication claimed an existing entry"));
    }
    Ok(())
}

/// C14: the denied save keeps the usable predecessor, and ordinary check
/// success or failure stands independently of the denial.
pub(crate) fn check_denied_save_keeps_predecessor() -> Result<(), Box<dyn Error>> {
    let predecessor = failure_generation(b"gc1-qualification-failure-predecessor")?;
    let current = failure_generation(b"gc1-qualification-failure-denied")?;
    if predecessor.cache_key() == current.cache_key() {
        return Err(invalid("denied generation merged with its predecessor"));
    }
    let job = failure_job("push", "refs/heads/main", 141, 1, 1412, "reader")?;
    let request = failure_discovery(
        job,
        current,
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![failure_entry(
            &predecessor,
            "refs/heads/main",
            1410,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision != DiscoveryDecision::PredecessorFallback {
        return Err(invalid("denied save dropped its predecessor"));
    }
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let retained = select_predecessor(&request, &visible)
        .ok_or_else(|| invalid("predecessor selection lost its candidate"))?;
    if retained.entry.cache_id != 1410 {
        return Err(invalid("predecessor selection kept the wrong entry"));
    }
    let report = failure_success_report()?;
    report.validate().map_err(session_error)?;
    if report.child_failed {
        return Err(invalid("denied save rewrote check success as failure"));
    }
    if report.dependency_compilations.value != Some(2) {
        return Err(invalid("denied save erased candidate compiler work"));
    }
    if report.native_compilations.value != Some(1) {
        return Err(invalid("denied save erased candidate native work"));
    }
    if report.origin_acquisitions.value != Some(2) {
        return Err(invalid("denied save erased candidate origin work"));
    }
    let failed_check = failure_ledger("check-failed", true, false)?;
    if failed_check.failed() == 0 {
        return Err(invalid("denied save hid an ordinary check failure"));
    }
    let denied = failure_save(
        false,
        Some("cache write denied by read-only fixture"),
        true,
        false,
        Some(1024),
        8192,
        SaveHalt::Running,
    );
    if decide_save(&denied)? != SaveOutcome::Denied {
        return Err(invalid("denied save escaped alongside check failure"));
    }
    Ok(())
}

/// C15: eviction, expiry, and deletion leave the entry absent; the absent
/// readback stays unavailable and never claims an existing entry, while the
/// degraded inventory reports unavailability instead of a false miss.
pub(crate) fn check_absent_states_stay_unavailable() -> Result<(), Box<dyn Error>> {
    let bytes = failure_frozen_bytes();
    let digest = sha256_hex(&bytes);
    let size = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let generation = failure_generation(b"gc1-qualification-failure-evicted")?;
    for inventory_complete in [true, false] {
        let verdict = decide_readback(&ReadbackInputs {
            frozen_archive_digest: &digest,
            frozen_archive_bytes: size,
            restored_bytes: Some(&bytes),
            inventory_complete,
            observed_entry: None,
            expected_key: &generation.cache_key(),
            expected_ref: "refs/heads/main",
            expected_version: "v1",
        })?;
        if !matches!(verdict, ReadbackVerdict::Unavailable { .. }) {
            return Err(invalid("absent entry claimed a readback verdict"));
        }
    }
    let job = failure_job("push", "refs/heads/main", 151, 1, 1511, "reader")?;
    let missing = failure_discovery(
        job.clone(),
        generation.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let missing_record = decide_discovery(&missing).map_err(|error| invalid(&error.to_string()))?;
    if missing_record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("evicted entry hid its miss"));
    }
    let degraded = failure_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        false,
    );
    let degraded_record =
        decide_discovery(&degraded).map_err(|error| invalid(&error.to_string()))?;
    if degraded_record.decision != DiscoveryDecision::ColdUnavailable {
        return Err(invalid("expired inventory reported a false miss"));
    }
    let cold = cold_ledger()?;
    require_nonzero(
        "absent-entry cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    must_fail(
        &require_nonzero("absent entry as warm pass", 0),
        "failed restore reported as a warm zero-work pass",
    )?;
    Ok(())
}
