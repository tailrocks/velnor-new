use std::{
    env,
    ffi::{OsStr, OsString},
    time::Duration,
};

use velnor_actions_mise::{PinnedTool, PinnedToolExec};

use super::{API_VERSION, GitHubApi, ImmutableResponse, JSON_LIMIT};
use crate::{OrchestratorError, internal::internal};

const API_TIMEOUT: Duration = Duration::from_secs(90);

impl GitHubApi {
    pub(super) fn verify_immutable_status(&self) -> Result<(), OrchestratorError> {
        let token = immutable_status_token(
            env::var("VELNOR_RELEASE_STATUS_TOKEN").ok().as_deref(),
            env::var("GH_TOKEN").ok().as_deref(),
            env::var("VELNOR_TAP_TOKEN").ok().as_deref(),
        )?;
        let mut args = vec![
            OsString::from("api"),
            OsString::from("-H"),
            OsString::from("Accept: application/vnd.github+json"),
            OsString::from("-H"),
            OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
        ];
        args.extend(crate::github_authority_read::arguments(
            &self.route("immutable-releases"),
        )?);
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .map_err(|_| internal("package_publish_gh_unavailable"))?;
        let result = exec
            .command_with_github_token(&self.catalog, &token)
            .and_then(|command| command.run_bounded(JSON_LIMIT, API_TIMEOUT))
            .map_err(|_| internal("package_publish_immutable_status_failed"))?;
        if !result.success {
            return Err(internal("package_publish_immutable_status_failed"));
        }
        let immutable: ImmutableResponse = serde_json::from_slice(&result.stdout)
            .map_err(|_| internal("package_publish_immutable_status_json"))?;
        if immutable.enabled != Some(true) {
            return Err(internal("package_publish_immutable_releases_disabled"));
        }
        Ok(())
    }
}

pub(super) fn immutable_status_token(
    status: Option<&str>,
    write_token: Option<&str>,
    tap_token: Option<&str>,
) -> Result<String, OrchestratorError> {
    let token = status
        .filter(|token| !token.is_empty())
        .ok_or_else(|| internal("package_publish_immutable_status_token_missing"))?;
    if write_token.is_some_and(|write| write == token) || tap_token.is_some_and(|tap| tap == token)
    {
        return Err(internal("package_publish_immutable_status_token_reused"));
    }
    Ok(token.to_owned())
}
