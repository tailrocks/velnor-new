// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support;
use std::{error::Error, fs};

pub(super) fn assert_all() -> Result<(), Box<dyn Error>> {
    for case in authority_rejection_cases() {
        reject_authority_case(case)?;
    }
    Ok(())
}

fn authority_rejection_cases() -> [support::RejectionCase; 40] {
    use support::RejectionCase as Case;
    [
        Case::WrongRepository,
        Case::WrongSelectedSource,
        Case::WrongCallerWorkflow,
        Case::WrongReusableWorkflow,
        Case::WrongRunAttempt,
        Case::WrongArtifact,
        Case::FailedLatestCi,
        Case::StaleLatestCi,
        Case::WrongAttestationDigest,
        Case::MissingAttestationSubject,
        Case::WrongRunnerLock,
        Case::FailedQualification,
        Case::BareRunnerJobName,
        Case::WrongRunnerJobCallerPrefix,
        Case::DuplicateRunnerJobName,
        Case::FailedRunnerJob,
        Case::ArchiveSubstitution,
        Case::WrongProducerEvent,
        Case::WrongProducerRef,
        Case::WrongProducerHead,
        Case::WrongQualifierSource,
        Case::WrongQualifierRef,
        Case::WrongQualifierWorkflow,
        Case::WrongQualifierSignerPath,
        Case::WrongQualifierSignerRef,
        Case::WrongQualifierSignerSha,
        Case::WrongQualifierAttempt,
        Case::WrongQualifierJobId,
        Case::FailedQualifierJob,
        Case::WrongProofSigner,
        Case::WrongProofSubjectDigest,
        Case::WrongProofSource,
        Case::WrongProofRef,
        Case::WrongProofLock,
        Case::WrongProofArchiveDigest,
        Case::WrongProofArchiveSize,
        Case::WrongProofOciIndex,
        Case::WrongProofPlatform,
        Case::WrongProofConfiguration,
        Case::WrongProofLayer,
    ]
}

fn reject_authority_case(case: support::RejectionCase) -> Result<(), Box<dyn Error>> {
    let fixture = support::Fixture::new_for(case.profile())?;
    fixture.apply_rejection(case)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let registry_before = fixture.preseed_registry_state()?;
    let output = if matches!(
        case,
        support::RejectionCase::WrongProofSigner
            | support::RejectionCase::MissingAttestationSubject
    ) {
        fixture.invoke_publication_boundary()?
    } else {
        fixture.invoke()?
    };
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        !output.status.success(),
        "accepted authority rejection {case:?}"
    );
    if let Some(expected) = expected_diagnostic(case) {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains(expected),
            "expected {expected} for {case:?}: {diagnostic}"
        );
    }
    assert!(!fixture.receipt.exists(), "receipt for {case:?}");
    assert_eq!(
        fixture.registry_state()?,
        registry_before,
        "registry for {case:?}"
    );
    assert_eq!(
        fs::read(&fixture.archive)?,
        archive_before,
        "archive for {case:?}"
    );
    assert_eq!(
        fs::read(&fixture.record)?,
        record_before,
        "record for {case:?}"
    );
    assert_eq!(
        fs::read(&foreign)?,
        b"preserve",
        "foreign file for {case:?}"
    );
    assert_eq!(
        fs::read_dir(&fixture.scratch)?.count(),
        1,
        "scratch for {case:?}"
    );
    assert_fixture_result(case, fixture_error.as_deref());
    Ok(())
}

fn assert_fixture_result(case: support::RejectionCase, error: Option<&str>) {
    if matches!(
        case,
        support::RejectionCase::WrongAttestationDigest
            | support::RejectionCase::WrongProofSubjectDigest
    ) {
        assert!(
            error.is_some_and(|value| value.contains("unattested subject")),
            "fixture did not reject requested attestation digest: {error:?}"
        );
    } else {
        assert!(
            error.is_none(),
            "fixture route failed for {case:?}: {error:?}"
        );
    }
}

fn expected_diagnostic(case: support::RejectionCase) -> Option<&'static str> {
    use support::RejectionCase as C;
    Some(match case {
        C::WrongRepository => "image_publication_repository_authority",
        C::WrongSelectedSource => "image_publication_record_request_mismatch",
        C::WrongCallerWorkflow
        | C::WrongRunAttempt
        | C::WrongProducerEvent
        | C::WrongProducerRef
        | C::WrongProducerHead => "image_publication_producer_run_identity",
        C::WrongArtifact => "image_publication_artifact_identity",
        C::FailedLatestCi | C::StaleLatestCi => "image_publication_ci_latest_not_success",
        C::WrongAttestationDigest | C::WrongProofSubjectDigest => return None,
        C::MissingAttestationSubject => "package_attestation_subject_inventory_mismatch",
        C::WrongRunnerLock => "runner_image_publication_lock_source_digest",
        C::ArchiveSubstitution => "image_publication_archive_attested_bytes",
        C::FailedQualification
        | C::WrongRunnerJobCallerPrefix
        | C::WrongQualifierSource
        | C::WrongQualifierRef
        | C::WrongQualifierWorkflow
        | C::WrongQualifierAttempt => "image_attestor_run_identity",
        C::WrongReusableWorkflow
        | C::WrongQualifierSignerPath
        | C::WrongQualifierSignerRef
        | C::WrongQualifierSignerSha => "image_qualification_signer_workflow",
        C::BareRunnerJobName
        | C::DuplicateRunnerJobName
        | C::FailedRunnerJob
        | C::WrongQualifierJobId
        | C::FailedQualifierJob => "image_qualification_proof_job",
        C::WrongProofSigner => "package_attestation_identity_mismatch",
        C::WrongProofSource | C::WrongProofLock => "image_qualification_proof_identity",
        C::WrongProofRef => "image_qualification_proof_invalid",
        C::WrongProofArchiveDigest
        | C::WrongProofArchiveSize
        | C::WrongProofOciIndex
        | C::WrongProofPlatform
        | C::WrongProofConfiguration
        | C::WrongProofLayer => "image_qualification_proof_candidate",
    })
}
