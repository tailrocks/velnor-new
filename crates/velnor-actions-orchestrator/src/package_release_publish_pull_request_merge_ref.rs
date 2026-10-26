use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, github};

#[derive(serde::Deserialize)]
struct PullRequestMergeCommit {
    sha: String,
    parents: Vec<PullRequestMergeParent>,
}

#[derive(serde::Deserialize)]
struct PullRequestMergeParent {
    sha: String,
}

pub(super) fn read_public_pull_request_merge_ref(
    api: &GitHubApi,
    number: u64,
    base_sha: &str,
    head_sha: &str,
    deadline: CheckDeadline,
) -> Result<Option<String>, OrchestratorError> {
    let route = api.route(&format!("git/matching-refs/pull/{number}/merge"));
    let first = exact_pull_request_merge_ref(api, &route, number)?;
    if let Some(sha) = first.as_deref() {
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        let commit: PullRequestMergeCommit =
            api.get_json(&api.route(&format!("git/commits/{sha}")))?;
        if commit.sha != sha
            || !github::is_lowercase_git_sha(&commit.sha)
            || commit.parents.len() != 2
            || commit.parents[0].sha != base_sha
            || commit.parents[1].sha != head_sha
            || !github::is_lowercase_git_sha(&commit.parents[0].sha)
            || !github::is_lowercase_git_sha(&commit.parents[1].sha)
        {
            return Err(internal(
                "package_public_pull_request_merge_commit_mismatch",
            ));
        }
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let second = exact_pull_request_merge_ref(api, &route, number)?;
    if first != second {
        return Err(internal("package_public_pull_request_merge_ref_changed"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(first)
}

fn exact_pull_request_merge_ref(
    api: &GitHubApi,
    route: &str,
    number: u64,
) -> Result<Option<String>, OrchestratorError> {
    let refs: Vec<github::GitRefResponse> = api.get_json(route)?;
    let expected = format!("refs/pull/{number}/merge");
    let exact = refs
        .into_iter()
        .filter(|reference| reference.ref_name == expected)
        .collect::<Vec<_>>();
    match exact.as_slice() {
        [] => Ok(None),
        [reference]
            if reference.object.object_type == "commit"
                && github::is_lowercase_git_sha(&reference.object.sha) =>
        {
            Ok(Some(reference.object.sha.clone()))
        }
        _ => Err(internal("package_public_pull_request_merge_ref_mismatch")),
    }
}
