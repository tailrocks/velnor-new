// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn completion_requires_observed_tool_pins_to_match_start() -> Result<(), Box<dyn std::error::Error>>
{
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut result = completion("0.1.1");
    result.post_publication_file_hashes.release_plz_sha256 = "4".repeat(64);
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&result)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn completion_requires_current_admitted_owners() -> Result<(), Box<dyn std::error::Error>> {
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    for owners in [
        Vec::new(),
        vec![JournalOwner {
            id: 2,
            login: "donbeave".into(),
            kind: JournalOwnerKind::User,
        }],
        vec![JournalOwner {
            id: 0,
            login: "donbeave".into(),
            kind: JournalOwnerKind::User,
        }],
        vec![
            JournalOwner {
                id: 1,
                login: "donbeave".into(),
                kind: JournalOwnerKind::User
            };
            2
        ],
    ] {
        let mut result = completion("0.1.1");
        result.packages[0].owners = owners;
        assert!(
            record_journal(
                &remote,
                &identity(),
                JournalStage::Completed,
                &receipt,
                Some(&result)
            )
            .is_err()
        );
        assert_eq!(remote.refs.borrow().len(), 1);
    }
    Ok(())
}

#[test]
fn start_then_complete_is_read_back_and_never_overwritten() -> Result<(), Box<dyn std::error::Error>>
{
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    let started = record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    assert_eq!(started, "refs/tags/velnor-receipt/42/1/publication-started");
    assert_eq!(
        record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?,
        started
    );
    let result = completion("0.1.1");
    let completed = record_journal(
        &remote,
        &identity(),
        JournalStage::Completed,
        &receipt,
        Some(&result),
    )?;
    assert_eq!(completed, "refs/tags/velnor-receipt/42/1/reconciled");
    assert_eq!(remote.refs.borrow().len(), 2);
    assert_eq!(
        inspect_journal(&remote, &identity(), JournalStage::Completed)?
            .ok_or("missing")?
            .completion,
        Some(result)
    );
    Ok(())
}

#[test]
fn ambiguous_write_and_unprotected_tags_do_not_authorize_release() {
    let receipt = receipt("0.1.1");
    let failing = Registry {
        fail_ref_after_write: true,
        ..Registry::default()
    };
    assert!(
        matches!(record_journal(&failing, &identity(), JournalStage::Started, &receipt, None), Err(error) if error.to_string().contains("crate_release_journal_ref_write_ambiguous"))
    );
    assert_eq!(failing.refs.borrow().len(), 1);
    assert_eq!(
        inspect_journal(&failing, &identity(), JournalStage::Started)
            .ok()
            .flatten()
            .map(|e| e.receipt),
        Some(receipt.clone())
    );
    assert!(record_journal(&failing, &identity(), JournalStage::Started, &receipt, None).is_ok());
    let mut before = Registry {
        fail_ref_before_write: Some(()),
        ..Registry::default()
    };
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_err());
    assert!(before.refs.borrow().is_empty());
    before.fail_ref_before_write = None;
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_ok());
    before.refs.borrow_mut().insert(
        "refs/tags/velnor-receipt/42/1/publication-started".into(),
        "e".repeat(40),
    );
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_err());
    let unprotected = Registry {
        reject_rules: true,
        ..Registry::default()
    };
    assert!(
        record_journal(
            &unprotected,
            &identity(),
            JournalStage::Started,
            &receipt,
            None
        )
        .is_err()
    );
    assert!(unprotected.blobs.borrow().is_empty());
    assert!(
        record_journal(
            &Registry::default(),
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&completion("0.1.1"))
        )
        .is_err()
    );
}

#[test]
fn inspection_rejects_cross_run_and_tampering() -> Result<(), Box<dyn std::error::Error>> {
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    assert!(inspect_journal(&remote, &identity(), JournalStage::Started)?.is_none());
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut other = identity();
    other.source_sha = "c".repeat(40);
    assert!(inspect_journal(&remote, &other, JournalStage::Started).is_err());
    let changed = self::receipt("0.1.2");
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &changed,
            Some(&completion("0.1.2"))
        )
        .is_err()
    );
    remote
        .blobs
        .borrow_mut()
        .values_mut()
        .next()
        .ok_or("blob")?
        .push(b' ');
    assert!(inspect_journal(&remote, &identity(), JournalStage::Started).is_err());
    Ok(())
}

#[test]
fn receipt_schema_rejects_unknown_fields_and_mismatched_results()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = receipt("0.1.1");
    let mut value = serde_json::to_value(&receipt)?;
    value.as_object_mut().ok_or("object")?.insert(
        "token".to_owned(),
        serde_json::Value::String("dummy".to_owned()),
    );
    assert!(serde_json::from_value::<JournalReceipt>(value).is_err());
    let remote = Registry::default();
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut result = completion("0.1.1");
    result.packages[0].downloaded_checksum = "f".repeat(64);
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&result)
        )
        .is_err()
    );
    assert_eq!(remote.refs.borrow().len(), 1);
    Ok(())
}

#[test]
fn semver_receipt_rejects_missing_forged_or_impossible_evidence() {
    use crate::crate_release_semver::SemverEvidence;
    let good = receipt("0.1.1");
    let mut bad = Vec::new();
    let mut missing = good.clone();
    missing.semver_evidence.clear();
    bad.push(missing);
    let mut unknown = good.clone();
    unknown.semver_evidence.insert(
        "other".into(),
        SemverEvidence::Deferred {
            predecessors: vec!["demo-crate".into()],
        },
    );
    bad.push(unknown);
    let mut digest = good.clone();
    digest.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::NoBaseline {
            checker_sha256: "a".repeat(64),
        },
    );
    bad.push(digest);
    let mut baseline = good.clone();
    baseline.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Checked {
            baseline: "0.1.1".into(),
            baseline_checksum: "d".repeat(64),
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        },
    );
    bad.push(baseline);
    let mut checksum = good.clone();
    checksum.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Checked {
            baseline: "0.1.0".into(),
            baseline_checksum: "A".repeat(64),
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        },
    );
    bad.push(checksum);
    let mut deferred = good;
    deferred.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Deferred {
            predecessors: vec!["demo-crate".into()],
        },
    );
    bad.push(deferred);
    let remote = Registry::default();
    for invalid in bad {
        assert!(
            record_journal(&remote, &identity(), JournalStage::Started, &invalid, None).is_err()
        );
    }
    assert!(remote.refs.borrow().is_empty());
}

#[test]
fn tag_object_sha_must_match_exact_git_object_bytes() {
    let remote = Registry {
        wrong_tag_sha: Some(()),
        ..Registry::default()
    };
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Started,
            &receipt("0.1.1"),
            None
        )
        .is_err()
    );
    assert!(remote.refs.borrow().is_empty());
}
