use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, github};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicRepositorySnapshot {
    pub(crate) id: u64,
    pub(crate) full_name: String,
    pub(crate) default_branch: String,
}

pub(crate) fn read_public_repository_snapshot(
    repository: &str,
    deadline: CheckDeadline,
) -> Result<PublicRepositorySnapshot, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository) {
        return Err(internal("package_public_repository_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    let observed: github::RepositoryResponse = api.get_json(&format!("repos/{repository}"))?;
    let (Some(full_name), Some(default_branch)) = (observed.full_name, observed.default_branch)
    else {
        return Err(internal("package_public_repository_identity_mismatch"));
    };
    if observed.id == 0 || full_name != repository || !valid_branch(&default_branch) {
        return Err(internal("package_public_repository_identity_mismatch"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(PublicRepositorySnapshot {
        id: observed.id,
        full_name,
        default_branch,
    })
}

fn valid_branch(branch: &str) -> bool {
    velnor_actions_contract::vcs::VcsInputs {
        commit: None,
        reference: Some(format!("refs/heads/{branch}")),
        submodules: std::collections::BTreeMap::new(),
    }
    .validate()
    .is_ok()
}
