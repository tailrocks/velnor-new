//! Installation scope and authenticated bot identity checks.

use super::github::{
    AppResponse, InstallationRepositoriesResponse, RepositoryResponse, UserResponse,
};
use super::{OrchestratorError, TapGitHubApi, internal};
use velnor_actions_contract::config::HomebrewTapConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BotIdentity {
    pub(super) login: String,
    pub(super) user_id: u64,
}

pub(super) fn verify_app_scope(
    api: &TapGitHubApi,
    policy: &HomebrewTapConfig,
    expected_app_id: u64,
    expected_installation_id: u64,
    slug: &str,
) -> Result<BotIdentity, OrchestratorError> {
    let app: AppResponse = api.get_json(&format!("apps/{slug}"))?;
    if app.id != expected_app_id || app.slug != slug {
        return Err(internal("tap_writer_app_identity_mismatch"));
    }
    let current_repo: RepositoryResponse = api.get_json(&format!("repos/{}", policy.repository))?;
    if current_repo.id == 0 || current_repo.full_name != policy.repository {
        return Err(internal("tap_writer_repository_identity_mismatch"));
    }
    if expected_installation_id == 0 {
        return Err(internal("tap_writer_installation_identity_mismatch"));
    }
    let repositories: InstallationRepositoriesResponse =
        api.get_json("installation/repositories?per_page=100&page=1")?;
    if repositories.total_count != 1
        || repositories.repositories.len() != 1
        || repositories.repositories[0].full_name != policy.repository
        || repositories.repositories[0].id != current_repo.id
    {
        return Err(internal("tap_writer_installation_scope_mismatch"));
    }
    let bot_login = format!("{slug}[bot]");
    let user: UserResponse = api.get_json(&format!("users/{bot_login}"))?;
    if user.login != bot_login || user.id == 0 {
        return Err(internal("tap_writer_bot_identity_mismatch"));
    }
    Ok(BotIdentity {
        login: bot_login,
        user_id: user.id,
    })
}
