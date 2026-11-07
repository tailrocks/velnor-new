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
    let output = fixture.invoke()?;
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        !output.status.success(),
        "accepted authority rejection {case:?}"
    );
    assert_producer_job_rejection(case, &output.stderr);
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

fn assert_producer_job_rejection(case: support::RejectionCase, stderr: &[u8]) {
    if case == support::RejectionCase::WrongRunnerJobCallerPrefix {
        let output = String::from_utf8_lossy(stderr);
        assert!(
            output.contains("image_attestor_run_identity"),
            "wrong qualifier run rejection for {case:?}: {output}"
        );
        return;
    }
    if matches!(
        case,
        support::RejectionCase::BareRunnerJobName
            | support::RejectionCase::DuplicateRunnerJobName
            | support::RejectionCase::FailedRunnerJob
    ) {
        let output = String::from_utf8_lossy(stderr);
        assert!(
            output.contains("image_qualification_proof_job"),
            "wrong producer job rejection for {case:?}: {output}"
        );
    }
}
