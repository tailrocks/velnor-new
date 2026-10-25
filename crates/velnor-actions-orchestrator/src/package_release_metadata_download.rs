use super::{ARTIFACT_LIMIT, GitHubApi};
use crate::OrchestratorError;
use std::ffi::OsString;

impl GitHubApi {
    pub(in super::super) fn verify_stable_tag_environment(
        &self,
        branch: &str,
    ) -> Result<(), OrchestratorError> {
        super::preflight::verify_stable_tag_environment(self, branch)
    }

    pub(in super::super) fn download_artifact_archive_bounded(
        &self,
        artifact_id: u64,
        limit: usize,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.request_accept(
            vec![OsString::from(
                self.route(&format!("actions/artifacts/{artifact_id}/zip")),
            )],
            limit.min(ARTIFACT_LIMIT),
            "application/vnd.github+json",
        )
    }
}
