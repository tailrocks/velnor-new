//! Native completion boundary; no authority is minted from parsed report flags.
use super::{LocalEvidenceStatus, Qualification, QualificationReason, evaluate, inputs};
use crate::cargo_artifact_capture::CargoCaptureReport;
use crate::dispatch_admission::AdmissionClosure;
use crate::dispatch_identity::NativeDispatchWitness;
use crate::session::completed_report::{SessionIdentity, prepare_directory};
use mbx_cache_core::{CompletedMeasurement, MeasurementPackageAvailability};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

/// Called only with original owning-session objects before report publication.
pub(crate) fn evaluate_native(
    directory: &Path,
    identity: &SessionIdentity,
    measurement: &CompletedMeasurement,
    capture: Option<&CargoCaptureReport>,
    dispatch: Option<&NativeDispatchWitness>,
    admission: Option<&AdmissionClosure>,
) -> Qualification {
    let snapshot = serde_json::json!({
        "schema_version":1,"completed":true,
        "mbx_version":crate::version::VERSION,
        "source_base_version":crate::version::SOURCE_BASE_VERSION,
        "identity":identity,"statistics":{"measurement":measurement},
    });
    let raw = serde_json::to_vec(&snapshot).unwrap_or_default();
    let receipts = load_receipts(directory, &identity.session_id);
    let diagnostics = capture
        .map(|capture| diagnostics(capture.stderr.bytes()))
        .unwrap_or_default();
    let mut result = evaluate(&raw, receipts.as_deref().unwrap_or_default(), &diagnostics);
    let mut reasons = native_context(
        &mut result,
        identity,
        measurement,
        capture,
        dispatch,
        receipts.is_ok(),
    );
    super::admission::apply(
        &mut result,
        &mut reasons,
        identity,
        measurement,
        dispatch,
        admission,
        receipts.as_deref().unwrap_or_default(),
    );
    result.reasons = reasons.into_iter().collect();
    if !result.reasons.is_empty() {
        result.admitted_evidence = None;
    }
    if result.reasons.is_empty() && result.admitted_evidence.is_some() {
        result.local_status = LocalEvidenceStatus::LocalEvidenceComplete;
    }
    if let Some(measurement) = &mut result.observed_measurement {
        measurement.coverage = result.coverage;
    }
    result
}

fn native_context(
    result: &mut Qualification,
    identity: &SessionIdentity,
    measurement: &CompletedMeasurement,
    capture: Option<&CargoCaptureReport>,
    dispatch: Option<&NativeDispatchWitness>,
    receipts_available: bool,
) -> BTreeSet<QualificationReason> {
    let mut reasons: BTreeSet<_> = result.reasons.iter().copied().collect();
    reasons.insert(QualificationReason::DispatchSnapshotPinningUnavailable);
    reasons.insert(QualificationReason::ManagedDispatchAdmissionClosureUnavailable);
    reasons.insert(QualificationReason::NativeUnitArtifactBindingUnavailable);
    if dispatch.is_some_and(NativeDispatchWitness::snapshot_pinning_verified) {
        reasons.remove(&QualificationReason::DispatchSnapshotPinningUnavailable);
    }
    // Native integrity has no claim about an externally qualified distribution.
    reasons.remove(&QualificationReason::QualifiedSourceAuthorityUnavailable);
    if dispatch.is_some_and(|witness| witness.validate_current().is_err()) {
        reasons.insert(QualificationReason::DispatchIdentityChanged);
        result.local_status = LocalEvidenceStatus::Unverified;
        result.coverage.status = mbx_cache_core::MeasurementCoverageStatus::Unverified;
        result.coverage.reason =
            Some(mbx_cache_core::MeasurementCoverageReason::DeliveryIncomplete);
    }
    if dispatch.is_some_and(|witness| {
        witness.identity_matches(&identity.session_id, &identity.root_session_id)
            && witness.snapshot_pinning_verified()
            && witness.behavior_abi() == crate::dispatch_identity::BEHAVIOR_ABI
    }) {
        reasons.remove(&QualificationReason::ManagedDispatchClosureUnavailable);
    }
    if !receipts_available {
        reasons.insert(QualificationReason::ReceiptDirectoryUnavailable);
    }
    if capture.is_some_and(|capture| {
        capture.identity_matches(&identity.session_id, &identity.root_session_id)
            && capture.local_capture_complete()
    }) {
        reasons.remove(&QualificationReason::DiagnosticCaptureClosureUnavailable);
        if capture.is_some_and(|capture| capture.protocol_complete) {
            reasons.remove(&QualificationReason::CargoArtifactClosureUnavailable);
            if capture.is_some_and(|capture| super::artifacts::agrees(measurement, capture)) {
                reasons.remove(&QualificationReason::NativeUnitArtifactBindingUnavailable);
            }
        }
    }
    if measurement.measurement_package_generation.is_none()
        || measurement.measurement_package_availability
            != Some(MeasurementPackageAvailability::Available)
    {
        reasons.insert(QualificationReason::PackageSnapshotUnavailable);
    } else if super::authority::packages_agree(measurement) {
        reasons.remove(&QualificationReason::UnitProvenanceAuthorityUnavailable);
    }
    // Sealed command/EOF and exact native-output-to-artifact capabilities are
    // required here. Public mutable report booleans cannot discharge them.
    reasons
}

#[cfg(test)]
mod tests {
    use super::diagnostics;

    #[test]
    fn unavailable_diagnostic_inside_forwarded_line_is_not_lost() {
        let raw = b"normal\nprefix MBX_MEASUREMENT_UNAVAILABLE {\"schema_version\":1}\n";
        assert_eq!(diagnostics(raw), vec![b"{\"schema_version\":1}".to_vec()]);
    }

    #[test]
    fn empty_stderr_does_not_create_a_diagnostic() {
        assert!(diagnostics(b"ordinary output\n").is_empty());
    }
}

fn load_receipts(directory: &Path, session_id: &str) -> eyre::Result<Vec<Vec<u8>>> {
    prepare_directory(directory)?;
    let prefix = format!("{session_id}.");
    let mut receipts = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(&prefix) || !name.contains(".measurement-") {
            continue;
        }
        if !["attempt", "ack", "failure"]
            .iter()
            .any(|stage| name.ends_with(&format!(".measurement-{stage}.json")))
        {
            eyre::bail!("measurement receipt has an unsupported filename");
        }
        if !entry.file_type()?.is_file() || receipts.len() >= inputs::MAX_RECEIPTS {
            eyre::bail!("measurement receipt directory contains unsupported entry");
        }
        let mut bytes = Vec::new();
        let file = std::fs::File::open(entry.path())?;
        validate_file(&file, &entry.path(), directory)?;
        file.take(inputs::MAX_RECEIPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > inputs::MAX_RECEIPT_BYTES {
            eyre::bail!("measurement receipt exceeds supported bound");
        }
        receipts.push(bytes);
    }
    Ok(receipts)
}

fn validate_file(file: &std::fs::File, path: &Path, directory: &Path) -> eyre::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let observed = file.metadata()?;
        let named = std::fs::symlink_metadata(path)?;
        let owner = std::fs::metadata(directory)?.uid();
        if !named.is_file()
            || !observed.is_file()
            || observed.uid() != owner
            || observed.mode() & 0o777 != 0o400
            || observed.nlink() != 1
            || observed.dev() != named.dev()
            || observed.ino() != named.ino()
        {
            eyre::bail!("measurement receipt is not an immutable owning file");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path, directory);
        eyre::bail!("measurement receipt privacy policy is unsupported");
    }
    #[cfg(unix)]
    Ok(())
}

fn diagnostics(stderr: &[u8]) -> Vec<Vec<u8>> {
    const PREFIX: &[u8] = b"MBX_MEASUREMENT_UNAVAILABLE ";
    stderr
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            line.windows(PREFIX.len())
                .position(|window| window == PREFIX)
                .map(|start| line[start + PREFIX.len()..].to_vec())
        })
        .collect()
}
