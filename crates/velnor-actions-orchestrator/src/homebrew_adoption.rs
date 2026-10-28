// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, ffi::OsString, path::Path};

use velnor_actions_contract::{
    WorkflowEvent,
    homebrew_adoption::{
        HomebrewAdoptionCandidateInput, HomebrewAdoptionEvidence, HomebrewAdoptionTransition,
        HomebrewAdoptionTreeFile, parse_homebrew_adoption_transition,
    },
};
use velnor_actions_mise::{CheckDeadline, GitRequest, TapTreeEntry};

use crate::{OrchestratorError, internal::internal};

const MAX_ADOPTION_FILE_BYTES: usize = 2 * 1024 * 1024;
#[path = "homebrew_adoption_diff.rs"]
mod diff;

use diff::{full_sha, read_transition_diff, safe_path};

#[derive(Debug, Clone)]
pub(crate) struct ProtectedHomebrewAdoptionTransition {
    transition: HomebrewAdoptionTransition,
}

impl ProtectedHomebrewAdoptionTransition {
    pub(crate) fn from_protected_source(bytes: &'static [u8]) -> Result<Self, OrchestratorError> {
        let transition = parse_homebrew_adoption_transition(bytes)
            .map_err(|_| internal("homebrew_adoption_transition_invalid"))?;
        Ok(Self { transition })
    }

    pub(crate) fn transition(&self) -> &HomebrewAdoptionTransition {
        &self.transition
    }
}

pub(crate) fn protected_homebrew_adoption_transition(
    tap_repository: &str,
    tap_repository_id: u64,
    authenticated_base_sha: &str,
) -> Result<ProtectedHomebrewAdoptionTransition, OrchestratorError> {
    let admitted = ProtectedHomebrewAdoptionTransition::from_protected_source(include_bytes!(
        "../../../.velnor/qualification/homebrew/jackin/transition.json"
    ))?;
    let transition = admitted.transition();
    if transition.tap_repository != tap_repository
        || transition.tap_repository_id != tap_repository_id
        || transition.base_sha != authenticated_base_sha
    {
        return Err(internal("homebrew_adoption_transition_not_admitted"));
    }
    Ok(admitted)
}

pub(crate) struct AdoptionValidationContext<'a> {
    pub(crate) root: &'a Path,
    pub(crate) admitted: &'a ProtectedHomebrewAdoptionTransition,
    pub(crate) tap_repository: &'a str,
    pub(crate) tap_repository_id: u64,
    pub(crate) event: WorkflowEvent,
    pub(crate) authenticated_base_sha: &'a str,
    pub(crate) authenticated_head_sha: &'a str,
    pub(crate) deadline: CheckDeadline,
}

pub(crate) fn validate_homebrew_adoption(
    context: &AdoptionValidationContext<'_>,
    generated_files: &BTreeMap<String, Vec<u8>>,
) -> Result<HomebrewAdoptionEvidence, OrchestratorError> {
    let transition = context.admitted.transition();
    transition
        .validate()
        .map_err(|_| internal("homebrew_adoption_transition_invalid"))?;
    if context.event != WorkflowEvent::PullRequest
        || context.tap_repository != transition.tap_repository
        || context.tap_repository_id != transition.tap_repository_id
        || context.authenticated_base_sha != transition.base_sha
        || !full_sha(context.authenticated_head_sha)
    {
        return Err(internal("homebrew_adoption_context_mismatch"));
    }

    let head_sha = git_value(context.root, "HEAD", context.deadline)?;
    if head_sha != context.authenticated_head_sha {
        return Err(internal("homebrew_adoption_head_mismatch"));
    }
    let base_tree = git_value(
        context.root,
        &format!("{}^{{tree}}", transition.base_sha),
        context.deadline,
    )?;
    let head_tree = git_value(context.root, "HEAD^{tree}", context.deadline)?;
    if base_tree != transition.base_tree {
        return Err(internal("homebrew_adoption_base_tree_mismatch"));
    }

    let candidate_tree =
        velnor_actions_mise::read_tap_tree(context.root, &head_sha, context.deadline)
            .map_err(|_| internal("homebrew_adoption_candidate_tree_invalid"))?;
    let changes = read_transition_diff(
        context.root,
        &transition.base_sha,
        &head_sha,
        context.deadline,
    )?;
    let base_claude_source = read_commit_file(
        context.root,
        &transition.base_sha,
        &transition.claude_source_path,
        context.deadline,
    )?;
    if crate::release_policy_admission::git_blob_sha1(&base_claude_source)
        .map_err(|_| internal("homebrew_adoption_source_blob_digest"))?
        != transition.claude_source_blob_sha
    {
        return Err(internal("homebrew_adoption_source_blob_mismatch"));
    }

    let candidate = read_candidate_files(
        context.root,
        transition,
        &head_sha,
        &candidate_tree,
        context.deadline,
    )?;
    let candidate_claude = candidate
        .bytes
        .get("CLAUDE.md")
        .ok_or_else(|| internal("homebrew_adoption_candidate_claude_missing"))?;
    let candidate_locator = candidate
        .bytes
        .get(velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
        .ok_or_else(|| internal("homebrew_adoption_candidate_locator_missing"))?;
    transition
        .validate_candidate_tree(HomebrewAdoptionCandidateInput {
            base: (&transition.base_sha, &base_tree),
            head: (&head_sha, &head_tree),
            changes: &changes,
            candidate_files: &candidate.tree_files,
            claude: candidate_claude,
            locator: candidate_locator,
            generated_files,
            candidate_file_bytes: &candidate.bytes,
        })
        .map_err(|_| internal("homebrew_adoption_candidate_invalid"))
}

pub(crate) fn validate_homebrew_adoption_output_base(
    root: &Path,
    admitted: &ProtectedHomebrewAdoptionTransition,
    generated_files: &BTreeMap<String, Vec<u8>>,
    output_base_sha: &str,
    deadline: CheckDeadline,
) -> Result<HomebrewAdoptionEvidence, OrchestratorError> {
    let transition = admitted.transition();
    transition
        .validate()
        .map_err(|_| internal("homebrew_adoption_transition_invalid"))?;
    if !full_sha(output_base_sha) || output_base_sha == transition.base_sha {
        return Err(internal("homebrew_adoption_output_base_invalid"));
    }
    let base_tree = git_value(root, &format!("{}^{{tree}}", transition.base_sha), deadline)?;
    let output_tree = git_value(root, &format!("{output_base_sha}^{{tree}}"), deadline)?;
    if base_tree != transition.base_tree {
        return Err(internal("homebrew_adoption_base_tree_mismatch"));
    }
    let output_files = velnor_actions_mise::read_tap_tree(root, output_base_sha, deadline)
        .map_err(|_| internal("homebrew_adoption_candidate_tree_invalid"))?;
    let changes = read_transition_diff(root, &transition.base_sha, output_base_sha, deadline)?;
    let base_claude = read_commit_file(
        root,
        &transition.base_sha,
        &transition.claude_source_path,
        deadline,
    )?;
    if crate::release_policy_admission::git_blob_sha1(&base_claude)
        .map_err(|_| internal("homebrew_adoption_source_blob_digest"))?
        != transition.claude_source_blob_sha
    {
        return Err(internal("homebrew_adoption_source_blob_mismatch"));
    }
    let output = read_candidate_files(root, transition, output_base_sha, &output_files, deadline)?;
    let locator_path =
        velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
    let candidate_claude = output
        .bytes
        .get("CLAUDE.md")
        .ok_or_else(|| internal("homebrew_adoption_candidate_claude_missing"))?;
    let candidate_locator = output
        .bytes
        .get(locator_path)
        .ok_or_else(|| internal("homebrew_adoption_candidate_locator_missing"))?;
    transition
        .validate_candidate_tree(HomebrewAdoptionCandidateInput {
            base: (&transition.base_sha, &base_tree),
            head: (output_base_sha, &output_tree),
            changes: &changes,
            candidate_files: &output.tree_files,
            claude: candidate_claude,
            locator: candidate_locator,
            generated_files,
            candidate_file_bytes: &output.bytes,
        })
        .map_err(|_| internal("homebrew_adoption_output_base_invalid"))
}

struct CandidateFiles {
    tree_files: Vec<HomebrewAdoptionTreeFile>,
    bytes: BTreeMap<String, Vec<u8>>,
}

fn read_candidate_files(
    root: &Path,
    transition: &HomebrewAdoptionTransition,
    commit: &str,
    tree: &[TapTreeEntry],
    deadline: CheckDeadline,
) -> Result<CandidateFiles, OrchestratorError> {
    let locator_path =
        velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
    let mut bytes_by_path = BTreeMap::new();
    for path in transition
        .generated_output_paths
        .iter()
        .map(String::as_str)
        .chain(["CLAUDE.md", locator_path])
    {
        let bytes = read_commit_file(root, commit, path, deadline)?;
        let entry = tree
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(|| internal("homebrew_adoption_candidate_file_missing"))?;
        if crate::release_policy_admission::git_blob_sha1(&bytes)
            .map_err(|_| internal("homebrew_adoption_candidate_blob_digest"))?
            != entry.blob_sha
        {
            return Err(internal("homebrew_adoption_candidate_blob_mismatch"));
        }
        bytes_by_path.insert(path.to_owned(), bytes);
    }
    Ok(CandidateFiles {
        tree_files: tree.iter().map(tree_file).collect(),
        bytes: bytes_by_path,
    })
}

fn tree_file(entry: &TapTreeEntry) -> HomebrewAdoptionTreeFile {
    HomebrewAdoptionTreeFile {
        path: entry.path.clone(),
        mode: entry.mode,
        blob_sha: entry.blob_sha.clone(),
    }
}

fn git_value(
    root: &Path,
    revision: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let valid_revision = matches!(revision, "HEAD" | "HEAD^{tree}")
        || full_sha(revision)
        || revision.strip_suffix("^{tree}").is_some_and(full_sha);
    if !valid_revision {
        return Err(internal("homebrew_adoption_revision_invalid"));
    }
    let output = GitRequest::rev_parse(vec![OsString::from(revision)])
        .run_in_until(root, deadline)
        .map_err(|_| internal("homebrew_adoption_git_rev_parse"))?;
    let output = output
        .require_success("git")
        .map_err(|_| internal("homebrew_adoption_git_rev_parse"))?;
    let value = output
        .stdout_text("git")
        .map_err(|_| internal("homebrew_adoption_git_revision_utf8"))?
        .trim()
        .to_owned();
    if !full_sha(&value) {
        return Err(internal("homebrew_adoption_git_revision_invalid"));
    }
    Ok(value)
}

pub(crate) fn read_commit_file(
    root: &Path,
    commit: &str,
    path: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    if !full_sha(commit) || !safe_path(path) {
        return Err(internal("homebrew_adoption_file_selection"));
    }
    let revision = format!("{commit}:{path}");
    let output = GitRequest::show(vec![OsString::from(revision)])
        .run_in_until(root, deadline)
        .map_err(|_| internal("homebrew_adoption_git_show"))?;
    let output = output
        .require_success("git")
        .map_err(|_| internal("homebrew_adoption_git_show"))?;
    if output.stdout.len() > MAX_ADOPTION_FILE_BYTES {
        return Err(internal("homebrew_adoption_file_too_large"));
    }
    Ok(output.stdout.clone())
}
