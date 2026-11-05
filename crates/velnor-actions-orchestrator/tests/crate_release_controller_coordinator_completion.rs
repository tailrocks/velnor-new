// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_journal::{
    JournalMutationPhase, inspect_journal_mutation, record_source_bound_start,
};

use super::{
    Cell, Github, JournalRegistryFake, OrchestratorError, Registry, fixture_archive,
    fixture_journal_plan, fs, journal_existing_owners, make_fixture, make_workspace_order_fixture,
    prepare_crate_release, reject_bad_completion_evidence, test_git,
    verify_crate_release_admission,
};

#[test]
pub(super) fn workspace_receipt_rederives_ordered_source_before_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let receipt = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(receipt.packages.len(), 2);
    verify_crate_release_admission(
        &receipt,
        &intent,
        &request,
        &github,
        &registry,
        crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution),
    )?;
    let mut reordered = receipt.clone();
    reordered.packages.swap(0, 1);
    assert!(
        verify_crate_release_admission(
            &reordered,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    let mut missing = receipt.clone();
    missing.packages.pop();
    assert!(
        verify_crate_release_admission(
            &missing,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    let mut wrong = request.clone();
    wrong.source_sha = "d".repeat(40);
    assert!(
        verify_crate_release_admission(
            &receipt,
            &intent,
            &wrong,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    fs::write(
        root.join("b/Cargo.toml"),
        "[package]\nname='forged'\nversion='0.1.1'\n",
    )?;
    assert!(
        verify_crate_release_admission(
            &receipt,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    Ok(())
}

#[test]
pub(super) fn serial_coordinator_records_upload_and_metadata_before_reconciling()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    let archive = fixture_archive(root)?;
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let calls = Cell::new(0);
    assert_semver_failure_writes_nothing(&input)?;
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |permit| {
            assert_eq!(permit.package, "demo");
            assert!(matches!(
                inspect_journal_mutation(&journal, &identity, 1)?
                    .expect("missing")
                    .phase,
                JournalMutationPhase::Intended
            ));
            assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
            calls.set(calls.get() + 1);
            *registry.archive.borrow_mut() = archive.clone();
            registry.absent.set(false);
            Ok(UploadAttempt::Uncertain)
        },
        || {},
    )?;
    assert_eq!(calls.get(), 1);
    assert_eq!(github.writes.get(), 2);
    assert_eq!(result.packages.len(), 1);
    assert_eq!(result.history.len(), 6);
    let completed = crate::crate_release_controller::reconciled_completion_packages(&result)?;
    assert_eq!(
        completed[0].owners,
        journal_existing_owners(&plan.packages[0].owners)
    );
    assert_eq!(
        completed[0].operations,
        vec![
            crate::crate_release_journal::JournalOperation::Published,
            crate::crate_release_journal::JournalOperation::TagCreated,
            crate::crate_release_journal::JournalOperation::ReleaseCreated,
        ]
    );
    assert_eq!(
        completed[0].registry_checksum,
        completed[0].downloaded_checksum
    );
    assert_eq!(completed[0].release_id, 12);
    assert!(
        completed[0]
            .files
            .iter()
            .any(|file| file.path == "src/lib.rs")
    );
    check_completion_retry(&input)?;
    Ok(())
}

pub(super) fn assert_semver_failure_writes_nothing(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::coordinate_release;
    let before = input.journal.write_calls.get();
    let missing_checker = tempfile::tempdir()?;
    assert!(
        crate::crate_release_semver::verify_before_upload(
            input.source,
            input.admission,
            input.registry,
            missing_checker.path(),
            "demo",
            test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    assert!(
        coordinate_release(
            input,
            |_| Err(OrchestratorError::Contract {
                problem: "semver_failed".into()
            }),
            |_| panic!("failed semver must not upload"),
            || {},
        )
        .is_err()
    );
    assert_eq!(input.journal.write_calls.get(), before);
    assert_eq!(input.github.writes.get(), 0);
    Ok(())
}

pub(super) fn check_completion_retry(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::coordinate_release;
    let mut wrong_pins = input.journal_receipt.expected_tools.clone();
    wrong_pins.rust_manifest_sha256 = "0".repeat(64);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(wrong_pins)
        )
        .is_err()
    );
    assert!(
        crate::crate_release_journal::inspect_journal(
            input.journal,
            input.identity,
            crate::crate_release_journal::JournalStage::Completed
        )?
        .is_none()
    );
    input.github.approved.set(false);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone()
            )
        )
        .is_err()
    );
    input.github.approved.set(true);
    reject_bad_completion_evidence(input)?;
    crate::crate_release_journal::record_reconciled_completion(
        input,
        crate::package_release_signer::fixture_measured_tools(
            input.journal_receipt.expected_tools.clone(),
        ),
    )?;
    let completion = crate::crate_release_journal::inspect_journal(
        input.journal,
        input.identity,
        crate::crate_release_journal::JournalStage::Completed,
    )?
    .expect("completion");
    assert_eq!(
        completion.completion.expect("result").packages[0].operations,
        vec![
            crate::crate_release_journal::JournalOperation::Published,
            crate::crate_release_journal::JournalOperation::TagCreated,
            crate::crate_release_journal::JournalOperation::ReleaseCreated,
        ]
    );
    let journal_writes = input.journal.write_calls.get();
    let retried = coordinate_release(
        input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("completed run must not upload"),
        || {},
    )?;
    assert_eq!(retried.history.len(), 6);
    crate::crate_release_journal::record_reconciled_completion(
        input,
        crate::package_release_signer::fixture_measured_tools(
            input.journal_receipt.expected_tools.clone(),
        ),
    )?;
    assert_eq!(input.github.writes.get(), 2);
    assert_eq!(input.journal.write_calls.get(), journal_writes);
    check_stale_completed_retry(input, journal_writes);
    Ok(())
}

pub(super) fn check_stale_completed_retry(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    journal_writes: usize,
) {
    use crate::crate_release_controller::coordinate_release;
    input.github.owner_match.set(false);
    assert!(
        coordinate_release(
            input,
            |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
            |_| panic!("completed run must not upload"),
            || {}
        )
        .is_err()
    );
    input.github.owner_match.set(true);
    let mut stale_pins = input.journal_receipt.expected_tools.clone();
    stale_pins.release_plz_sha256 = "0".repeat(64);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(stale_pins)
        )
        .is_err()
    );
    assert_eq!(input.journal.write_calls.get(), journal_writes);
    assert_eq!(input.github.writes.get(), 2);
}
