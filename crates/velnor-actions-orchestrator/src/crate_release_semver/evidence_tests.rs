// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{SemverEvidence, VerifiedSemver};

fn proof() -> VerifiedSemver {
    VerifiedSemver {
        package: "demo".into(),
        version: "0.1.1".into(),
        source_sha: "a".repeat(40),
        intent_sha256: "b".repeat(64),
        evidence: SemverEvidence::Checked {
            baseline: "0.1.0".into(),
            baseline_checksum: "d".repeat(64),
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        },
    }
}

#[test]
fn preflight_revalidation_accepts_only_verified_deferred_progress() {
    use std::collections::BTreeMap;
    let deferred = SemverEvidence::Deferred {
        predecessors: vec!["dep".into()],
    };
    let checked = proof().evidence;
    let map = |v| BTreeMap::from([("demo".to_owned(), v)]);
    assert!(super::matches_revalidated_preflight(
        &map(deferred.clone()),
        &map(checked.clone())
    ));
    assert!(super::matches_revalidated_preflight(
        &map(deferred.clone()),
        &map(deferred)
    ));
    assert!(!super::matches_revalidated_preflight(
        &map(checked.clone()),
        &BTreeMap::new()
    ));
    let mut changed_checksum = checked.clone();
    if let SemverEvidence::Checked {
        baseline_checksum, ..
    } = &mut changed_checksum
    {
        *baseline_checksum = "e".repeat(64);
    }
    assert!(!super::matches_revalidated_preflight(
        &map(checked.clone()),
        &map(changed_checksum)
    ));
    let mut changed = checked.clone();
    if let SemverEvidence::Checked { baseline, .. } = &mut changed {
        *baseline = "0.0.9".into();
    }
    assert!(!super::matches_revalidated_preflight(
        &map(checked),
        &map(changed)
    ));
}

#[test]
fn final_proof_is_bound_to_package_source_and_intent() {
    assert!(
        proof()
            .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"b".repeat(64))
            .is_ok()
    );
    assert!(
        proof()
            .into_bound_evidence("other", "0.1.1", &"a".repeat(40), &"b".repeat(64))
            .is_err()
    );
    assert!(
        proof()
            .into_bound_evidence("demo", "0.2.0", &"a".repeat(40), &"b".repeat(64))
            .is_err()
    );
    assert!(
        proof()
            .into_bound_evidence("demo", "0.1.1", &"f".repeat(40), &"b".repeat(64))
            .is_err()
    );
    assert!(
        proof()
            .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"f".repeat(64))
            .is_err()
    );
    let mut deferred = proof();
    deferred.evidence = SemverEvidence::Deferred {
        predecessors: vec!["other".into()],
    };
    assert!(
        deferred
            .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"b".repeat(64))
            .is_err()
    );
}
