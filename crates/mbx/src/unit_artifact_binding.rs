//! Native successful compiler output observations. No filename convention or
//! Cargo artifact absence can mint an output observation.

use crate::process_measurement::Invocation;
use mbx_cache_core::{CacheDigest, FileIdentity, MeasurementEvent, OutputObservation};
use mbx_cache_rustc::RustcOutputs;
use std::fmt;
use std::path::{Path, PathBuf};

const MAX_OUTPUTS: usize = 4096;
const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug)]
pub(crate) enum NativeOutputObservationError {
    MissingMandatoryOutput(PathBuf),
    EvidenceUnavailable(String),
}

impl NativeOutputObservationError {
    pub(crate) fn is_missing_mandatory_output(&self) -> bool {
        matches!(self, Self::MissingMandatoryOutput(_))
    }
}

impl fmt::Display for NativeOutputObservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingMandatoryOutput(path) => {
                write!(
                    formatter,
                    "mandatory compiler output missing: {}",
                    path.display()
                )
            }
            Self::EvidenceUnavailable(reason) => {
                write!(formatter, "native output evidence unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for NativeOutputObservationError {}

/// Called only after the native owner observed successful compilation or
/// verified cache restoration, before any executable shim replacement.
/// Validate the entire inventory before delivering any observations. Dep-info
/// is intentionally outside Cargo's public compiler-artifact output inventory.
pub(crate) fn observe<S: FnMut(MeasurementEvent)>(
    measurement: &mut Invocation<S>,
    outputs: &RustcOutputs,
) -> Result<(), NativeOutputObservationError> {
    if outputs.files.is_empty() || outputs.files.len() > MAX_OUTPUTS {
        return Err(unavailable("output inventory outside observation bounds"));
    }
    let observations = outputs
        .files
        .iter()
        .map(|path| observe_file(path, measurement.outcome()))
        .collect::<Result<Vec<_>, _>>()?;
    for observation in observations {
        measurement.record_output(observation);
    }
    Ok(())
}

fn observe_file(
    path: &Path,
    cache_outcome: mbx_cache_core::CacheOutcome,
) -> Result<OutputObservation, NativeOutputObservationError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            NativeOutputObservationError::MissingMandatoryOutput(path.to_path_buf())
        } else {
            unavailable("cannot inspect mandatory output")
        }
    })?;
    if !metadata.is_file() {
        return Err(NativeOutputObservationError::MissingMandatoryOutput(
            path.to_path_buf(),
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| unavailable_or_missing(path, "cannot canonicalize output"))?;
    if canonical.as_os_str().len() > MAX_PATH_BYTES {
        return Err(unavailable("output path outside observation bounds"));
    }
    let identity = FileIdentity::for_digest_cache(&canonical, &metadata)
        .map_err(|_| unavailable_or_missing(&canonical, "cannot establish output identity"))?
        .ok_or_else(|| unavailable("output identity unavailable"))?;
    let digest = CacheDigest::blake3_file(&canonical)
        .map_err(|_| unavailable_or_missing(&canonical, "cannot hash actual output"))?;
    if !identity
        .still_describes()
        .map_err(|_| unavailable_or_missing(&canonical, "cannot recheck output identity"))?
    {
        return Err(unavailable_or_missing(
            &canonical,
            "output changed during observation",
        ));
    }
    Ok(OutputObservation {
        path: canonical,
        aliases: Vec::new(),
        cache_outcome,
        digest,
        file_identity: Some(identity),
    })
}

fn unavailable(reason: &str) -> NativeOutputObservationError {
    NativeOutputObservationError::EvidenceUnavailable(reason.to_owned())
}

fn unavailable_or_missing(path: &Path, reason: &str) -> NativeOutputObservationError {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            NativeOutputObservationError::MissingMandatoryOutput(path.to_path_buf())
        }
        Ok(metadata) if !metadata.is_file() => {
            NativeOutputObservationError::MissingMandatoryOutput(path.to_path_buf())
        }
        _ => unavailable(reason),
    }
}

/// A raw file relation, not a completeness or provenance capability. The
/// owning native gate must separately validate original capture, dispatch and
/// source-managed measurement authority. Copies and guessed aliases never join.
pub(crate) fn matches_current_output(observation: &OutputObservation, filename: &Path) -> bool {
    let Ok(canonical) = filename.canonicalize() else {
        return false;
    };
    if canonical != observation.path {
        return false;
    }
    observe_file(&canonical, observation.cache_outcome).is_ok_and(|current| {
        observation.file_identity.is_some()
            && current.file_identity == observation.file_identity
            && current.digest == observation.digest
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mbx_cache_core::CacheOutcome;

    #[test]
    fn exact_output_rejects_copy_and_replacement() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("output");
        let copy = temporary.path().join("copy");
        std::fs::write(&output, b"actual compiler bytes").unwrap();
        let observation = observe_file(&output, CacheOutcome::Miss).unwrap();
        assert!(matches_current_output(&observation, &output));
        std::fs::copy(&output, &copy).unwrap();
        assert!(!matches_current_output(&observation, &copy));
        std::fs::rename(&output, temporary.path().join("original")).unwrap();
        std::fs::write(&output, b"actual compiler bytes").unwrap();
        assert!(!matches_current_output(&observation, &output));
        std::fs::write(&output, b"replaced executable shim").unwrap();
        assert!(!matches_current_output(&observation, &output));
    }

    #[test]
    fn mandatory_missing_or_nonregular_output_fails() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing");
        assert!(
            observe_file(&missing, CacheOutcome::Hit)
                .unwrap_err()
                .is_missing_mandatory_output()
        );
        assert!(
            observe_file(temporary.path(), CacheOutcome::Hit)
                .unwrap_err()
                .is_missing_mandatory_output()
        );
    }
}
