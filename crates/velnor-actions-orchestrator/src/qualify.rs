//! Candidate qualification: artifact execution, never a rebuild.

use crate::OrchestratorError;

/// Staged path of the downloaded candidate binary under runner temp.
pub(crate) const CANDIDATE_BINARY_PATH: &str = "$RUNNER_TEMP/velnor/candidate/velnor-actions";

/// Directory receiving the artifact-executed generation check.
const QUALIFY_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/qualify-check";

/// Payload fragments that prove a rebuild; qualify argv must avoid them all.
const REBUILD_MARKERS: [&str; 5] = ["cargo", "mbx", "rustc", "mise", "build"];

/// Fixed artifact-only qualification argv over the staged candidate.
///
/// Runs the downloaded binary's `plan` plus `generate --check` without
/// invoking any build tool; any rebuild marker fails closed (boot §4).
///
/// # Errors
///
/// Returns a contract error when the vector carries a rebuild marker.
pub fn qualify_argv_staged() -> Result<Vec<String>, OrchestratorError> {
    QualifyRequest::staged().argv()
}

/// Typed candidate-qualification request: artifact execution, never a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QualifyRequest {
    /// Downloaded candidate binary path (staged by download-artifact).
    binary: String,
    /// Scratch directory for the artifact-executed generation check.
    output_dir: String,
}

impl QualifyRequest {
    /// Fixed request over the staged candidate path.
    pub(crate) fn staged() -> Self {
        Self {
            binary: CANDIDATE_BINARY_PATH.to_owned(),
            output_dir: QUALIFY_OUTPUT_DIR.to_owned(),
        }
    }

    /// Fixed qualify argv: artifact `plan` plus artifact `generate --check`.
    ///
    /// Runs the downloaded binary twice (plan, then a generation check
    /// into scratch) without invoking any build tool. Rejects rebuilds.
    pub(crate) fn argv(&self) -> Result<Vec<String>, OrchestratorError> {
        let script = format!(
            "{} plan && {} generate --output-dir {} && test -f {}/.github/workflows/ci.yml",
            self.binary, self.binary, self.output_dir, self.output_dir
        );
        let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
        Self::check_no_rebuild(&argv)?;
        Ok(argv)
    }

    /// Reject any qualify vector that could rebuild the candidate.
    pub(crate) fn check_no_rebuild(argv: &[String]) -> Result<(), OrchestratorError> {
        for arg in argv {
            let lower = arg.to_lowercase();
            if REBUILD_MARKERS.iter().any(|mark| lower.contains(mark)) {
                return Err(OrchestratorError::Contract {
                    problem: format!("qualify_must_not_rebuild:{arg}"),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualify_runs_artifact_without_rebuild_markers() {
        let argv = QualifyRequest::staged()
            .argv()
            .map_err(|err| err.to_string());
        assert!(argv.as_ref().is_ok_and(|argv| argv[0] == "sh"));
        assert!(argv.is_ok_and(|argv| {
            argv[2].contains("plan")
                && argv[2].contains("generate")
                && QualifyRequest::check_no_rebuild(&argv).is_ok()
        }));
    }

    #[test]
    fn qualify_rebuild_attempt_rejected() {
        for bad in [
            "cargo test",
            "mbx build",
            "rustc x",
            "mise exec",
            "rebuild all",
        ] {
            let err = QualifyRequest::check_no_rebuild(&["sh".to_owned(), bad.to_owned()]);
            assert!(
                err.is_err_and(|err| err.to_string().contains("must_not_rebuild")),
                "{bad}"
            );
        }
    }
}
