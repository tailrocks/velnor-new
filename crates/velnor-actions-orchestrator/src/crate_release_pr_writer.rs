// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::{OrchestratorError, internal::internal};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeEntry {
    mode: String,
    sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Commit {
    sha: String,
    tree: String,
    parent: String,
    message: String,
    author_name: String,
    author_email: String,
    committer_name: String,
    committer_email: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PullRequest {
    number: u64,
    repository: String,
    head_repository: String,
    head: String,
    head_sha: String,
    base: String,
    author_id: u64,
    state: String,
    title: String,
    body: String,
}

struct Plan {
    repository: String,
    base: String,
    source_sha: String,
    base_tree: String,
    expected_tree: String,
    branch: String,
    message: String,
    title: String,
    body: String,
    author_id: u64,
    allowed_paths: BTreeSet<String>,
    changes: BTreeMap<String, (String, Vec<u8>)>,
}

pub(crate) fn write_prepared_pr(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    preparation: &crate::crate_release_pr_acquisition::VerifiedPreparation,
) -> Result<(), OrchestratorError> {
    let trees = crate::crate_release_pr_tree::GitHubTrees {
        api: github,
        repository: &preparation.artifact.binding.repository,
    };
    let source = &preparation.artifact.binding.source_sha;
    let plan = build_plan(
        &trees,
        &preparation.intent,
        source,
        &preparation.source_tree,
        &preparation.artifact,
    )?;
    create_or_reconcile(&NativeWriter { github }, &plan)?;
    Ok(())
}

fn build_plan<R: crate::crate_release_pr_tree::TreeRead>(
    trees: &R,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    source_sha: &str,
    source_tree_sha: &str,
    artifact: &crate::crate_release_pr_artifact::PreparationArtifact,
) -> Result<Plan, OrchestratorError> {
    let invalid = || internal("release_pr_plan_invalid");
    if !crate::release_identity::same_repository(&artifact.binding.repository, &intent.repository)
        || artifact.binding.source_sha != source_sha
        || artifact.binding.workflow_sha != source_sha
        || artifact.binding.run_attempt != 1
        || artifact.binding.run_id == 0
    {
        return Err(invalid());
    }
    let allowed_paths: BTreeSet<_> = intent.pr_write_paths.iter().cloned().collect();
    let mut changes = BTreeMap::new();
    let mut overlay = BTreeMap::new();
    for file in &artifact.files {
        if !allowed_paths.contains(&file.path) {
            return Err(invalid());
        }
        let mode =
            crate::crate_release_pr_tree::regular_file_mode(trees, source_tree_sha, &file.path)?;
        let sha = crate::release_policy_admission::git_blob_sha1(&file.bytes)?;
        if changes
            .insert(file.path.clone(), (mode.clone(), file.bytes.clone()))
            .is_some()
        {
            return Err(invalid());
        }
        overlay.insert(
            file.path.clone(),
            crate::crate_release_pr_tree::Change { mode, sha },
        );
    }
    let expected_tree =
        crate::crate_release_pr_tree::overlay_tree(trees, source_tree_sha, &overlay)?;
    let plan = Plan {
        repository: artifact.binding.repository.clone(),
        base: intent.default_branch.clone(),
        source_sha: source_sha.to_owned(),
        base_tree: source_tree_sha.to_owned(),
        expected_tree,
        branch: format!("release-plz-{}-{}", artifact.binding.run_id, artifact.binding.run_attempt),
        message: "chore: release".to_owned(),
        title: "chore: release".to_owned(),
        body: "Automated release preparation. Review the version, dependency and changelog changes before merging.".to_owned(),
        author_id: 41_898_282,
        allowed_paths,
        changes,
    };
    validate_plan(&plan)?;
    Ok(plan)
}

trait WriterIo {
    fn require_not_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError>;
    fn require_no_other_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError>;
    fn base_sha(&self, plan: &Plan) -> Result<String, OrchestratorError>;
    fn branch_sha(&self, plan: &Plan) -> Result<Option<String>, OrchestratorError>;
    fn commit(&self, plan: &Plan, sha: &str) -> Result<Commit, OrchestratorError>;
    fn pull_requests(&self, plan: &Plan) -> Result<Vec<PullRequest>, OrchestratorError>;
    fn create_blob(&self, plan: &Plan, bytes: &[u8]) -> Result<String, OrchestratorError>;
    fn create_tree(
        &self,
        plan: &Plan,
        changes: &BTreeMap<String, TreeEntry>,
    ) -> Result<String, OrchestratorError>;
    fn create_commit(&self, plan: &Plan, tree: &str) -> Result<String, OrchestratorError>;
    fn create_branch(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError>;
    fn create_pull_request(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError>;
}

fn create_or_reconcile<I: WriterIo>(io: &I, plan: &Plan) -> Result<u64, OrchestratorError> {
    validate_plan(plan)?;
    require_base(io, plan)?;
    let expected = expected_tree(plan)?;
    let existing = io.branch_sha(plan)?;
    let head = if let Some(sha) = existing {
        verify_commit(io, plan, &sha)?
    } else {
        if !io.pull_requests(plan)?.is_empty() {
            return Err(internal("release_pr_conflict"));
        }
        create_branch(io, plan, &expected)?
    };
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    if let Some(pr) = exact_pull_request(io, plan, &head)? {
        return Ok(pr);
    }
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    io.require_not_release_pr(plan)?;
    io.require_no_other_release_pr(plan)?;
    drop(io.create_pull_request(plan, &head));
    let pr = exact_pull_request(io, plan, &head)?
        .ok_or_else(|| internal("release_pr_create_unreconciled"))?;
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    Ok(pr)
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_plan(plan: &Plan) -> Result<(), OrchestratorError> {
    let Some((owner, repository)) = plan.repository.split_once('/') else {
        return Err(internal("release_pr_plan_invalid"));
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repository)
        || !velnor_actions_contract::is_valid_branch_name(&plan.base)
        || !velnor_actions_contract::is_valid_branch_name(&plan.branch)
        || !plan.branch.starts_with("release-plz-")
        || !valid_sha(&plan.source_sha)
        || !valid_sha(&plan.base_tree)
        || !valid_sha(&plan.expected_tree)
        || plan.base_tree == plan.expected_tree
        || plan.author_id == 0
        || plan.message.is_empty()
        || plan.title.is_empty()
        || plan.body.is_empty()
        || plan
            .changes
            .keys()
            .any(|path| !plan.allowed_paths.contains(path))
    {
        return Err(internal("release_pr_plan_invalid"));
    }
    Ok(())
}

fn require_branch<I: WriterIo>(
    io: &I,
    plan: &Plan,
    expected: &str,
) -> Result<(), OrchestratorError> {
    if io.branch_sha(plan)?.as_deref() != Some(expected) {
        return Err(internal("release_pr_branch_moved"));
    }
    Ok(())
}

fn require_base<I: WriterIo>(io: &I, plan: &Plan) -> Result<(), OrchestratorError> {
    if io.base_sha(plan)? != plan.source_sha {
        return Err(internal("release_pr_base_moved"));
    }
    Ok(())
}

fn expected_tree(plan: &Plan) -> Result<BTreeMap<String, TreeEntry>, OrchestratorError> {
    let mut expected = BTreeMap::new();
    if plan.changes.is_empty() {
        return Err(internal("release_pr_empty"));
    }
    for (path, (mode, bytes)) in &plan.changes {
        if !matches!(mode.as_str(), "100644" | "100755") {
            return Err(internal("release_pr_file_mode"));
        }
        let entry = TreeEntry {
            mode: mode.clone(),
            sha: crate::release_policy_admission::git_blob_sha1(bytes)?,
        };
        expected.insert(path.clone(), entry);
    }
    Ok(expected)
}

fn create_branch<I: WriterIo>(
    io: &I,
    plan: &Plan,
    expected: &BTreeMap<String, TreeEntry>,
) -> Result<String, OrchestratorError> {
    let mut changes = BTreeMap::new();
    for (path, (mode, bytes)) in &plan.changes {
        require_base(io, plan)?;
        let sha = io.create_blob(plan, bytes)?;
        if crate::release_policy_admission::git_blob_sha1(bytes)? != sha {
            return Err(internal("release_pr_blob_mismatch"));
        }
        changes.insert(
            path.clone(),
            TreeEntry {
                mode: mode.clone(),
                sha,
            },
        );
    }
    require_base(io, plan)?;
    let tree = io.create_tree(plan, &changes)?;
    if !valid_sha(&tree) || tree != plan.expected_tree || &changes != expected {
        return Err(internal("release_pr_tree_mismatch"));
    }
    require_base(io, plan)?;
    let sha = io.create_commit(plan, &tree)?;
    verify_commit(io, plan, &sha)?;
    require_base(io, plan)?;
    io.require_not_release_pr(plan)?;
    io.require_no_other_release_pr(plan)?;
    if io.branch_sha(plan)?.is_some() || !io.pull_requests(plan)?.is_empty() {
        return Err(internal("release_pr_conflict"));
    }
    require_base(io, plan)?;
    drop(io.create_branch(plan, &sha));
    let actual = io
        .branch_sha(plan)?
        .ok_or_else(|| internal("release_pr_branch_unreconciled"))?;
    verify_commit(io, plan, &actual)
}

fn verify_commit<I: WriterIo>(io: &I, plan: &Plan, sha: &str) -> Result<String, OrchestratorError> {
    if !valid_sha(sha) {
        return Err(internal("release_pr_branch_conflict"));
    }
    let commit = io.commit(plan, sha)?;
    let email = format!(
        "{}+github-actions[bot]@users.noreply.github.com",
        plan.author_id
    );
    if commit.sha != sha
        || commit.parent != plan.source_sha
        || commit.message != plan.message
        || commit.tree != plan.expected_tree
        || commit.author_name != "github-actions[bot]"
        || commit.committer_name != "github-actions[bot]"
        || commit.author_email != email
        || commit.committer_email != email
    {
        return Err(internal("release_pr_branch_conflict"));
    }
    Ok(sha.to_owned())
}

fn exact_pull_request<I: WriterIo>(
    io: &I,
    plan: &Plan,
    sha: &str,
) -> Result<Option<u64>, OrchestratorError> {
    let pulls = io.pull_requests(plan)?;
    if pulls.len() > 1 {
        return Err(internal("release_pr_duplicate"));
    }
    let Some(pr) = pulls.first() else {
        return Ok(None);
    };
    if pr.number == 0
        || pr.repository != plan.repository
        || pr.head_repository != plan.repository
        || pr.head != plan.branch
        || pr.head_sha != sha
        || pr.base != plan.base
        || pr.author_id != plan.author_id
        || pr.state != "open"
        || pr.title != plan.title
        || pr.body != plan.body
    {
        return Err(internal("release_pr_conflict"));
    }
    Ok(Some(pr.number))
}

#[path = "crate_release_pr_writer_native.rs"]
mod native;
use native::NativeWriter;

#[cfg(test)]
#[path = "crate_release_pr_writer_tests.rs"]
mod tests;
