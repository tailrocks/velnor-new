// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NewRegistry, fixture_archive, make_fixture, raw_prepare_crate_release, test_git};
use crate::crate_release_controller::CrateReleaseAdmissionReceipt;
use crate::crate_release_semver::{SemverEvidence, verify_observed_evidence, verify_preflight};
use std::cell::Cell;

#[test]
fn existing_first_version_derives_metadata_evidence_without_checker_or_baseline()
-> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let tools = tempfile::tempdir()?;
    let (intent, request, github, base) = make_fixture(source.path())?;
    github.approved.set(true);
    base.absent.set(false);
    *base.archive.borrow_mut() = fixture_archive(source.path())?;
    let registry = NewRegistry {
        base,
        visible: Cell::new(true),
        owner_changed: Cell::new(false),
    };
    for (tag, release) in [(false, 4), (true, 4), (true, 0)] {
        github.tag_exists.set(tag);
        github.release_variant.set(release);
        let mut admission = raw_prepare_crate_release(
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(
                source.path(),
                test_git(),
                &crate::UnprivilegedSourceExecution,
            ),
        )?;
        assert!(matches!(
            (&admission.packages[0].action, tag, release),
            (
                crate::crate_release_controller::CrateReleaseAction::RepairTag { .. },
                false,
                4
            ) | (
                crate::crate_release_controller::CrateReleaseAction::RepairRelease { .. },
                true,
                4
            ) | (
                crate::crate_release_controller::CrateReleaseAction::Reconciled { .. },
                true,
                0
            )
        ));
        admission.semver_evidence = verify_preflight(
            source.path(),
            &admission,
            &registry,
            tools.path(),
            test_git(),
            &crate::UnprivilegedSourceExecution,
        )?;
        let evidence = admission.semver_evidence.get("demo").ok_or("evidence")?;
        assert!(
            matches!(evidence, SemverEvidence::MetadataOnly { package, version, source_sha, target_checksum, .. }
            if package == "demo" && version == "0.1.1" && source_sha == &request.source_sha
            && target_checksum == &crate::generator::sha256_hex(&registry.base.archive.borrow()))
        );
        verify_observed_evidence(&admission, &admission)?;
        reject_evidence_substitution(&admission);
        reject_metadata_drift(&admission)?;
    }
    assert_eq!(std::fs::read_dir(tools.path())?.count(), 0);
    Ok(())
}

fn reject_evidence_substitution(admission: &CrateReleaseAdmissionReceipt) {
    let checker_sha256 = "a".repeat(64);
    for replacement in [
        SemverEvidence::NoBaseline {
            checker_sha256: checker_sha256.clone(),
        },
        SemverEvidence::Checked {
            baseline: "0.1.0".into(),
            baseline_checksum: "b".repeat(64),
            checker_sha256: checker_sha256.clone(),
        },
        SemverEvidence::Deferred {
            predecessors: vec!["other".into()],
        },
        SemverEvidence::PriorNoBaselinePending {
            checker_sha256,
            target_checksum: "c".repeat(64),
        },
    ] {
        let mut changed = admission.clone();
        changed.semver_evidence.insert("demo".into(), replacement);
        assert!(verify_observed_evidence(&changed, admission).is_err());
    }
    let mut changed = admission.clone();
    changed.semver_evidence.clear();
    assert!(verify_observed_evidence(&changed, admission).is_err());
}

fn reject_metadata_drift(
    admission: &CrateReleaseAdmissionReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    for field in [
        "package",
        "version",
        "source_sha",
        "intent_sha256",
        "target_checksum",
    ] {
        let mut value = serde_json::to_value(admission)?;
        value["semver_evidence"]["demo"][field] = serde_json::json!("altered");
        let changed: CrateReleaseAdmissionReceipt = serde_json::from_value(value)?;
        assert!(
            verify_observed_evidence(&changed, admission).is_err(),
            "{field}"
        );
    }
    let mut changed = admission.clone();
    changed.intent.packages.push("unselected".into());
    assert!(verify_observed_evidence(admission, &changed).is_err());
    let mut changed = admission.clone();
    changed.packages[0].owners = crate::crate_release_registry::CrateRegistryOwnership::NewCrate;
    assert!(verify_observed_evidence(admission, &changed).is_err());
    let mut changed = admission.clone();
    changed.packages[0].source.version = "0.1.2".into();
    assert!(verify_observed_evidence(admission, &changed).is_err());
    let mut changed = admission.clone();
    changed.packages[0].action = crate::crate_release_controller::CrateReleaseAction::Publish;
    assert!(verify_observed_evidence(admission, &changed).is_err());
    Ok(())
}
