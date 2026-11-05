// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionHttp, OrchestratorError};

impl GitHubAdmissionHttp {
    pub fn qualification_job_log(
        &self,
        repository: &str,
        job_id: u64,
    ) -> Result<Vec<u8>, OrchestratorError> {
        crate::api::github_evidence_http::qualification_job_log(
            repository,
            job_id,
            &self.token,
            std::time::Instant::now() + self.timeout,
        )
    }

    pub fn qualification_artifact_bytes(
        &self,
        repository: &str,
        artifact_id: u64,
        expected_size: u64,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        crate::api::github_evidence_http::qualification_artifact_bytes(
            repository,
            artifact_id,
            expected_size,
            expected_sha256,
            &self.token,
            std::time::Instant::now() + self.timeout,
        )
    }
}
