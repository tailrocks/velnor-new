// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, crate_release_git::ReleaseGit, internal::internal};
use std::path::Path;
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_mise::{CheckDeadline, GitRequest};

pub(crate) trait HistoryWatch {
    fn check(&self) -> Result<(), ()>;
}
pub(crate) struct DirectHistoryWatch;
impl HistoryWatch for DirectHistoryWatch {
    fn check(&self) -> Result<(), ()> {
        CheckDeadline::check_scope().map_err(|_| ())
    }
}

fn read_historical_intent(
    root: &Path,
    commit: &str,
    git: &ReleaseGit,
    deadline: CheckDeadline,
    wire: &dyn HistoryWatch,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    let invalid = || internal("crate_release_historical_intent_invalid");
    wire.check().map_err(|()| invalid())?;
    verify_object_store(root, git, deadline, wire)?;
    let commit_bytes = object(root, "commit", commit, git, deadline, wire, 1_048_576)?;
    let first = commit_bytes
        .split(|byte| *byte == b'\n')
        .next()
        .ok_or_else(invalid)?;
    let tree = std::str::from_utf8(first.strip_prefix(b"tree ").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let root_tree = object(root, "tree", tree, git, deadline, wire, 16_777_216)?;
    let github = tree_entry(&root_tree, ".github", "40000")?;
    let github_tree = object(root, "tree", &github, git, deadline, wire, 16_777_216)?;
    let intent = tree_entry(&github_tree, "velnor-release-intent.toml", "100644")?;
    let bytes = object(root, "blob", &intent, git, deadline, wire, 1_048_576)?;
    let original: CrateReleaseIntent =
        toml::from_str(std::str::from_utf8(&bytes).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if !matches!(original.runner.as_str(), "ubuntu-24.04" | "ubuntu-26.04") {
        return Err(invalid());
    }
    let mut validated = original.clone();
    "ubuntu-26.04".clone_into(&mut validated.runner);
    validated.validate().map_err(|_| invalid())?;
    verify_object_store(root, git, deadline, wire)?;
    wire.check().map_err(|()| invalid())?;
    Ok(original)
}

fn object(
    root: &Path,
    kind: &str,
    id: &str,
    git: &ReleaseGit,
    deadline: CheckDeadline,
    wire: &dyn HistoryWatch,
    cap: usize,
) -> Result<Vec<u8>, OrchestratorError> {
    let invalid = || internal("crate_release_historical_object_invalid");
    wire.check().map_err(|()| invalid())?;
    let output = GitRequest::object(kind, id)
        .map_err(|_| invalid())?
        .run_release_in_bounded(root, git, cap, deadline.remaining().map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    wire.check().map_err(|()| invalid())?;
    if !output.success
        || crate::crate_release_qualification_snapshot::object::object_sha(kind, &output.stdout)?
            != id
    {
        return Err(invalid());
    }
    Ok(output.stdout)
}

fn tree_entry(bytes: &[u8], wanted: &str, wanted_mode: &str) -> Result<String, OrchestratorError> {
    let invalid = || internal("crate_release_historical_tree_invalid");
    let mut remaining = bytes;
    let mut names = std::collections::BTreeSet::new();
    let mut found = None;
    while !remaining.is_empty() {
        let end = remaining
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?;
        if remaining.len() < end + 21 {
            return Err(invalid());
        }
        let header = std::str::from_utf8(&remaining[..end]).map_err(|_| invalid())?;
        let (mode, name) = header.split_once(' ').ok_or_else(invalid)?;
        if name.is_empty() || name.contains('/') || !names.insert(name.to_owned()) {
            return Err(invalid());
        }
        if name == wanted {
            if mode != wanted_mode {
                return Err(invalid());
            }
            found = Some(
                remaining[end + 1..end + 21]
                    .iter()
                    .flat_map(|byte| {
                        const HEX: &[u8; 16] = b"0123456789abcdef";
                        [
                            char::from(HEX[usize::from(byte >> 4)]),
                            char::from(HEX[usize::from(byte & 15)]),
                        ]
                    })
                    .collect(),
            );
        }
        remaining = &remaining[end + 21..];
    }
    found.ok_or_else(invalid)
}

fn verify_object_store(
    root: &Path,
    git: &ReleaseGit,
    deadline: CheckDeadline,
    wire: &dyn HistoryWatch,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_historical_store_invalid");
    wire.check().map_err(|()| invalid())?;
    for path in [
        ".",
        ".git",
        ".git/objects",
        ".git/info",
        ".git/objects/info",
        ".git/objects/pack",
    ] {
        let metadata = std::fs::symlink_metadata(root.join(path)).map_err(|_| invalid())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid());
        }
    }
    for path in [
        ".git/shallow",
        ".git/info/grafts",
        ".git/objects/info/alternates",
        ".git/objects/info/http-alternates",
    ] {
        match std::fs::symlink_metadata(root.join(path)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(invalid()),
        }
    }
    for entry in std::fs::read_dir(root.join(".git/objects/pack")).map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        if entry.file_name().to_string_lossy().ends_with(".promisor") {
            return Err(invalid());
        }
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid());
        }
        deadline.remaining().map_err(|_| invalid())?;
    }
    let config = std::fs::symlink_metadata(root.join(".git/config")).map_err(|_| invalid())?;
    if !config.is_file() || config.file_type().is_symlink() {
        return Err(invalid());
    }
    wire.check().map_err(|()| invalid())?;
    let result = GitRequest::new(
        "config",
        vec![
            "--local".into(),
            "--no-includes".into(),
            "--get-regexp".into(),
            "^(extensions\\.partialclone|remote\\..*\\.promisor|include\\.|includeif\\.)".into(),
        ],
    )
    .map_err(|_| invalid())?
    .run_release_in_bounded(
        root,
        git,
        1_048_576,
        deadline.remaining().map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    wire.check().map_err(|()| invalid())?;
    if result.code != Some(1) || !result.stdout.is_empty() {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct TransitionReceipts<'a> {
    pub prior: &'a crate::crate_release_journal::JournalReceipt,
    pub current: &'a crate::crate_release_journal::JournalReceipt,
    pub current_intent: &'a CrateReleaseIntent,
}

pub(crate) fn verify_runner_transition(
    root: &Path,
    receipts: TransitionReceipts<'_>,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    git: &ReleaseGit,
    deadline: CheckDeadline,
    wire: &dyn HistoryWatch,
) -> Result<(), OrchestratorError> {
    let TransitionReceipts {
        prior,
        current,
        current_intent,
    } = receipts;
    let invalid = || internal("crate_release_historical_transition_invalid");
    current_intent.validate().map_err(|_| invalid())?;
    if current_intent.runner != "ubuntu-26.04" {
        return Err(invalid());
    }
    let current_is_live = current.identity.workflow_sha == request.workflow_sha
        && current.identity.run_id == request.run_id
        && current.identity.run_attempt == request.run_attempt;
    let adjacent_intent = if current_is_live {
        verify_receipt_intent(current, current_intent)?;
        current_intent.clone()
    } else {
        let historical_current =
            read_historical_intent(root, &current.identity.workflow_sha, git, deadline, wire)?;
        if !matches!(
            historical_current.runner.as_str(),
            "ubuntu-24.04" | "ubuntu-26.04"
        ) {
            return Err(invalid());
        }
        verify_receipt_intent(current, &historical_current)?;
        crate::crate_release_github::verify_predecessor_authority(
            github,
            request,
            current,
            &historical_current,
        )?;
        verify_historical_tools(&historical_current, current, deadline)?;
        historical_current
    };
    let historical =
        read_historical_intent(root, &prior.identity.workflow_sha, git, deadline, wire)?;
    if !matches!(historical.runner.as_str(), "ubuntu-24.04" | "ubuntu-26.04") {
        return Err(invalid());
    }
    verify_receipt_intent(prior, &historical)?;
    if normalized_historical_base(&historical, &adjacent_intent)? != current.base_intent_sha256 {
        return Err(invalid());
    }
    crate::crate_release_github::verify_predecessor_authority(github, request, prior, &historical)?;
    verify_historical_tools(&historical, prior, deadline)?;
    Ok(())
}

fn verify_historical_tools(
    intent: &CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_historical_tools_invalid");
    let (manifest, bundle) =
        crate::release::package_release_publish::read_historical_sdk_attestation_assets(
            &intent.generator_version,
            &intent.generator_source_sha,
            &intent.generator_manifest_sha256,
            deadline,
        )?;
    let dir = tempfile::tempdir().map_err(|_| invalid())?;
    let manifest_path = dir.path().join("release-manifest.json");
    let bundle_path = dir.path().join("release-manifest.json.intoto.jsonl");
    std::fs::write(&manifest_path, manifest).map_err(|_| invalid())?;
    std::fs::write(&bundle_path, bundle).map_err(|_| invalid())?;
    crate::release::package_release_signer::verify_historical_tool_expectations(
        &manifest_path,
        &bundle_path,
        intent,
        receipt,
        deadline,
    )
}

fn normalized_historical_base(
    historical: &CrateReleaseIntent,
    current_intent: &CrateReleaseIntent,
) -> Result<String, OrchestratorError> {
    let mut normalized = historical.clone();
    normalized.runner.clone_from(&current_intent.runner);
    normalized
        .rust_version
        .clone_from(&current_intent.rust_version);
    normalized
        .generator_version
        .clone_from(&current_intent.generator_version);
    normalized
        .generator_source_sha
        .clone_from(&current_intent.generator_source_sha);
    normalized
        .generator_manifest_sha256
        .clone_from(&current_intent.generator_manifest_sha256);
    normalized
        .release_plz_version
        .clone_from(&current_intent.release_plz_version);
    normalized
        .required_workflow_path
        .clone_from(&current_intent.required_workflow_path);
    crate::crate_release_journal::release_base_intent_sha256(&normalized)
}

fn verify_receipt_intent(
    receipt: &crate::crate_release_journal::JournalReceipt,
    intent: &CrateReleaseIntent,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_historical_receipt_invalid");
    let bytes = velnor_actions_contract::canonical_json_bytes(intent).map_err(|_| invalid())?;
    let predecessor = intent.journal_predecessor.as_ref().map(|item| {
        crate::crate_release_journal::JournalIdentity {
            repository: receipt.identity.repository.clone(),
            source_sha: receipt.identity.source_sha.clone(),
            workflow_sha: item.workflow_sha.clone(),
            run_id: item.run_id,
            run_attempt: item.run_attempt,
        }
    });
    if receipt.generator_version != intent.generator_version
        || receipt.release_plz_version != intent.release_plz_version
        || receipt.rust_version != intent.rust_version
        || receipt.trusted_reviewer_id != intent.trusted_reviewer_id
        || crate::planner::generator::sha256_hex(&bytes) != receipt.intent_sha256
        || crate::crate_release_journal::release_base_intent_sha256(intent)?
            != receipt.base_intent_sha256
        || predecessor != receipt.predecessor
        || !crate::release::release_identity::same_repository(
            &intent.repository,
            &receipt.identity.repository,
        )
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn verify_prior_bound(
    root: &Path,
    prior: &crate::crate_release_journal::JournalReceipt,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    git: &ReleaseGit,
    deadline: CheckDeadline,
    wire: &dyn HistoryWatch,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    let intent = read_historical_intent(root, &prior.identity.workflow_sha, git, deadline, wire)?;
    verify_receipt_intent(prior, &intent)?;
    crate::crate_release_github::verify_predecessor_authority(github, request, prior, &intent)?;
    verify_historical_tools(&intent, prior, deadline)?;
    Ok(intent)
}

#[cfg(test)]
#[path = "crate_release_historical_intent_tests.rs"]
mod direct_tests;
