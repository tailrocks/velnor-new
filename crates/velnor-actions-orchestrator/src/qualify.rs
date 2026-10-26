// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;

pub(crate) const CANDIDATE_BINARY_PATH: &str = "$RUNNER_TEMP/velnor/candidate/velnor-actions";

const QUALIFY_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/qualify-check";

const REBUILD_MARKERS: [&str; 5] = ["cargo", "mbx", "rustc", "mise", "build"];

pub fn qualify_argv_staged() -> Result<Vec<String>, OrchestratorError> {
    QualifyRequest::staged().argv()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QualifyRequest {
    binary: String,
    output_dir: String,
}

impl QualifyRequest {
    pub(crate) fn staged() -> Self {
        Self {
            binary: CANDIDATE_BINARY_PATH.to_owned(),
            output_dir: QUALIFY_OUTPUT_DIR.to_owned(),
        }
    }

    pub(crate) fn argv(&self) -> Result<Vec<String>, OrchestratorError> {
        let script = format!(
            "{} plan && {} generate --output-dir {} && test -f {}/.github/workflows/ci.yml",
            self.binary, self.binary, self.output_dir, self.output_dir
        );
        let argv = vec!["sh".to_owned(), "-c".to_owned(), script];
        Self::check_no_rebuild(&argv)?;
        Ok(argv)
    }

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
