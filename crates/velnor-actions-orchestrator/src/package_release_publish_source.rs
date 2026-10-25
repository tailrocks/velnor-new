use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, github, release_policy_admission};

pub(crate) fn verify_preview_main_tip(
    repository: &str,
    repository_id: u64,
    branch: &str,
) -> Result<String, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository) {
        return Err(internal("package_publish_preview_tip_repository_invalid"));
    }
    github::GitHubApi::new(repository.to_owned()).verify_preview_main_tip(repository_id, branch)
}

pub(crate) fn verify_public_protected_main_lineage(
    repository: &str,
    repository_id: u64,
    branch: &str,
    source_sha: &str,
    workflow_authority_sha: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository) || repository_id == 0 {
        return Err(internal("package_public_repository_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    let tip = github::verify_public_preview_main_ancestry(
        &api,
        repository_id,
        branch,
        source_sha,
        workflow_authority_sha,
    )?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(tip)
}

pub(crate) fn verify_public_pull_request_lineage(
    repository: &str,
    repository_id: u64,
    branch: &str,
    base_sha: &str,
    head_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository) || repository_id == 0 {
        return Err(internal("package_public_repository_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    github::verify_public_pull_request_lineage(&api, repository_id, branch, base_sha, head_sha)?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(())
}

pub(crate) fn read_protected_source_file(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    source_sha: &str,
    path: &str,
    max_bytes: usize,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    try_read_protected_source_file(
        repository,
        repository_id,
        default_branch,
        source_sha,
        path,
        max_bytes,
        deadline,
    )?
    .ok_or_else(|| internal("package_public_source_file_missing"))
}

pub(crate) fn try_read_protected_source_file(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    source_sha: &str,
    path: &str,
    max_bytes: usize,
    deadline: CheckDeadline,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || default_branch.is_empty()
        || default_branch.len() > 255
        || default_branch.bytes().any(|byte| byte.is_ascii_control())
        || !github::is_lowercase_git_sha(source_sha)
        || path.is_empty()
        || max_bytes == 0
        || max_bytes
            > velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES
    {
        return Err(internal("package_public_source_file_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_preview_repository_identity(repository_id, default_branch)?;
    let mut source = release_policy_admission::SourceFiles::new(&api, source_sha)?;
    let bytes = source.read_bounded_if_present(path, max_bytes)?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedSourceTreeIdentity {
    pub(crate) tree_sha: String,
    pub(crate) regular_files: Vec<ProtectedSourceTreeEntry>,
    pub(crate) nonregular_entries: Vec<ProtectedSourceTreeEntry>,
    pub(crate) directories: Vec<ProtectedSourceTreeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedSourceTreeEntry {
    pub(crate) path: String,
    pub(crate) mode: String,
    pub(crate) sha: String,
}

pub(crate) fn read_protected_source_tree_identity(
    repository: &str,
    repository_id: u64,
    commit_sha: &str,
    deadline: CheckDeadline,
) -> Result<ProtectedSourceTreeIdentity, OrchestratorError> {
    read_protected_source_tree_identity_inner(repository, repository_id, commit_sha, None, deadline)
}

pub(crate) fn read_protected_adoption_seed_base_tree_identity(
    repository: &str,
    repository_id: u64,
    commit_sha: &str,
    symlink_path: &str,
    symlink_blob_sha: &str,
    deadline: CheckDeadline,
) -> Result<ProtectedSourceTreeIdentity, OrchestratorError> {
    read_protected_source_tree_identity_inner(
        repository,
        repository_id,
        commit_sha,
        Some((symlink_path, symlink_blob_sha)),
        deadline,
    )
}

fn read_protected_source_tree_identity_inner(
    repository: &str,
    repository_id: u64,
    commit_sha: &str,
    allowed_symlink: Option<(&str, &str)>,
    deadline: CheckDeadline,
) -> Result<ProtectedSourceTreeIdentity, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || !github::is_lowercase_git_sha(commit_sha)
    {
        return Err(internal("package_public_source_tree_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_repository_identity(repository_id)?;
    let source = release_policy_admission::SourceFiles::new(&api, commit_sha)?;
    let tree_sha = source.root_tree_sha().to_owned();
    let regular_files = source.regular_files()?;
    let nonregular_entries = source.nonregular_entries()?;
    let directories = source.directory_entries()?;
    let complete = regular_files
        .iter()
        .chain(&nonregular_entries)
        .chain(&directories)
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: &entry.path,
            mode: &entry.mode,
            kind: source_entry_kind(&entry.mode),
            sha: &entry.sha,
        })
        .collect::<Vec<_>>();
    if let Some((path, blob_sha)) = allowed_symlink {
        crate::homebrew::tree_validation::validate_protected_symlink_base(
            &complete, 100_000, path, blob_sha,
        )?;
    } else {
        crate::homebrew::tree_validation::validate(&complete, 100_000)?;
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(ProtectedSourceTreeIdentity {
        tree_sha,
        regular_files: regular_files.iter().map(protected_tree_entry).collect(),
        nonregular_entries: nonregular_entries
            .iter()
            .map(protected_tree_entry)
            .collect(),
        directories: directories.iter().map(protected_tree_entry).collect(),
    })
}

fn protected_tree_entry(
    entry: &release_policy_admission::source_files::SourceFileEntry,
) -> ProtectedSourceTreeEntry {
    ProtectedSourceTreeEntry {
        path: entry.path.clone(),
        mode: entry.mode.clone(),
        sha: entry.sha.clone(),
    }
}

fn source_entry_kind(mode: &str) -> &'static str {
    match mode {
        "040000" => "tree",
        "160000" => "commit",
        _ => "blob",
    }
}
