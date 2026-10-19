//! Pinned, deadline-bound preview archive download to a caller-owned descriptor.
use super::{API_TIMEOUT, API_VERSION, GitHubApi, JSON_LIMIT};
use crate::{OrchestratorError, internal::internal};
use std::ffi::{OsStr, OsString};
use velnor_actions_mise::{PinnedTool, PinnedToolExec};
impl GitHubApi {
    /// Preview-only streaming download; existing bounded response callers retain their limits.
    pub(super) fn request_to_file(
        &self,
        route: String,
        output: &mut std::fs::File,
        size: u64,
        deadline: velnor_actions_mise::CheckDeadline,
    ) -> Result<(), OrchestratorError> {
        #[cfg(test)]
        if self.fixture.is_some() {
            return Err(internal("unexpected_fixture_transport_request"));
        }
        let args = vec![
            OsString::from("api"),
            OsString::from("-H"),
            OsString::from("Accept: application/vnd.github+json"),
            OsString::from("-H"),
            OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
            OsString::from(route),
        ];
        let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .and_then(|exec| exec.command(&self.catalog))
            .map_err(|_| internal("package_publish_gh_unavailable"))?;
        let result = velnor_actions_mise::CheckDeadline::after(API_TIMEOUT)
            .and_then(|api_deadline| {
                api_deadline
                    .scope(|| command.run_stdout_to_file_until(output, size, JSON_LIMIT, deadline))
                    .and_then(|result| result)
            })
            .map_err(|_| internal("package_publish_api_failed"))?;
        if !result.success || result.stdout_bytes != size {
            return Err(internal("preview_artifact_download_identity"));
        }
        Ok(())
    }
}
