//! Small identity, digest, and environment helpers for the isolated tap writer.

use std::{env, fs, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use velnor_actions_contract::config::HomebrewTapConfig;

use super::github::{BlobResponse, CommitResponse, RefResponse, TapGitHubApi, TreeResponse};

use sha2::{Digest, Sha256};

use crate::{OrchestratorError, internal::internal, tap_prepare::HomebrewTapPreparationProof};

pub(super) fn verify_repository_base(
    api: &TapGitHubApi,
    policy: &HomebrewTapConfig,
    proof: &HomebrewTapPreparationProof,
) -> Result<String, OrchestratorError> {
    if policy.repository != proof.tap_repository {
        return Err(internal("tap_writer_target_mismatch"));
    }
    let current: RefResponse = api.get_json(&api.repo_route("git/ref/heads/main"))?;
    if current.object.object_type != "commit" || current.object.sha != proof.base_sha {
        return Err(internal("tap_writer_stale_tap_main"));
    }
    Ok(current.object.sha)
}

pub(super) struct FormulaBase {
    pub(super) bytes: Vec<u8>,
    pub(super) tree_sha: String,
}

pub(super) fn read_formula_base(
    api: &TapGitHubApi,
    formula: &str,
    base_sha: &str,
) -> Result<FormulaBase, OrchestratorError> {
    let commit: CommitResponse =
        api.get_json(&api.repo_route(&format!("git/commits/{base_sha}")))?;
    if commit.sha != base_sha || !is_full_sha(&commit.tree.sha) {
        return Err(internal("tap_writer_base_commit_invalid"));
    }
    let tree: TreeResponse =
        api.get_json(&api.repo_route(&format!("git/trees/{}?recursive=1", commit.tree.sha)))?;
    if tree.truncated || tree.sha != commit.tree.sha {
        return Err(internal("tap_writer_tree_truncated"));
    }
    validate_regular_tree(&tree)?;
    let mut entries = tree.tree.iter().filter(|entry| entry.path == formula);
    let entry = entries
        .next()
        .ok_or_else(|| internal("tap_writer_formula_missing"))?;
    if entries.next().is_some() || entry.mode != "100644" || entry.object_type != "blob" {
        return Err(internal("tap_writer_formula_mode_invalid"));
    }
    let blob: BlobResponse = api.get_json(&api.repo_route(&format!("git/blobs/{}", entry.sha)))?;
    if blob.sha != entry.sha || blob.encoding != "base64" {
        return Err(internal("tap_writer_formula_blob_invalid"));
    }
    let bytes = STANDARD
        .decode(blob.content.replace('\n', ""))
        .map_err(|_| internal("tap_writer_formula_blob_encoding"))?;
    if bytes.len() > 1024 * 1024
        || crate::release_policy_admission::git_blob_sha1(&bytes)? != entry.sha
    {
        return Err(internal("tap_writer_formula_blob_digest_mismatch"));
    }
    Ok(FormulaBase {
        bytes,
        tree_sha: tree.sha,
    })
}

fn validate_regular_tree(tree: &TreeResponse) -> Result<(), OrchestratorError> {
    let entries = tree
        .tree
        .iter()
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: &entry.path,
            mode: &entry.mode,
            kind: &entry.object_type,
            sha: &entry.sha,
        })
        .collect::<Vec<_>>();
    crate::homebrew::tree_validation::validate(&entries, 50_000)
}

pub(super) fn branch_name(
    receipt_digest: &str,
    base_sha: &str,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| is_sha256(value))
        .ok_or_else(|| internal("tap_writer_branch_identity_invalid"))?;
    if !is_full_sha(base_sha) {
        return Err(internal("tap_writer_branch_identity_invalid"));
    }
    let channel = match channel {
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
    };
    Ok(format!("velnor/{channel}/{digest}-{base_sha}"))
}

pub(super) fn is_full_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn required_env(key: &str) -> Result<String, OrchestratorError> {
    let value = env::var(key).map_err(|_| internal("tap_writer_env_missing"))?;
    if value.is_empty() || value.len() > 4096 {
        return Err(internal("tap_writer_env_invalid"));
    }
    Ok(value)
}

pub(super) fn parse_decimal_id(value: &str) -> Result<u64, OrchestratorError> {
    let id = value
        .parse::<u64>()
        .map_err(|_| internal("tap_writer_id_invalid"))?;
    if id == 0 || id.to_string() != value {
        return Err(internal("tap_writer_id_invalid"));
    }
    Ok(id)
}

pub(super) fn safe_app_slug(value: &str) -> Result<&str, OrchestratorError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(internal("tap_writer_app_slug_invalid"));
    }
    Ok(value)
}

pub(super) fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

#[cfg(test)]
mod regular_tree_tests {
    use super::*;
    use crate::tap_prepare::writer::github::TreeEntry;

    fn entry(path: &str, mode: &str, object_type: &str) -> TreeEntry {
        TreeEntry {
            path: path.to_owned(),
            mode: mode.to_owned(),
            object_type: object_type.to_owned(),
            sha: "a".repeat(40),
        }
    }

    fn tree(entries: Vec<TreeEntry>) -> TreeResponse {
        TreeResponse {
            sha: "b".repeat(40),
            truncated: false,
            tree: entries,
        }
    }

    #[test]
    fn complete_tree_rejects_nonformula_links_and_submodules() {
        let formula = || entry("Formula/jackin-preview.rb", "100644", "blob");
        let directory = || entry("Formula", "040000", "tree");
        assert!(
            validate_regular_tree(&tree(vec![
                directory(),
                formula(),
                entry("AGENTS.md", "100644", "blob")
            ]))
            .is_ok()
        );
        for invalid in [
            entry("CLAUDE.md", "120000", "blob"),
            entry("vendor", "160000", "commit"),
            entry("x", "100644", "tree"),
        ] {
            assert!(validate_regular_tree(&tree(vec![directory(), formula(), invalid])).is_err());
        }
    }

    #[test]
    fn complete_tree_rejects_duplicate_paths_and_non_directory_parents() {
        assert!(
            validate_regular_tree(&tree(vec![
                entry("x", "100644", "blob"),
                entry("x", "100755", "blob")
            ]))
            .is_err()
        );
        assert!(
            validate_regular_tree(&tree(vec![
                entry("x", "100644", "blob"),
                entry("x/y", "100644", "blob")
            ]))
            .is_err()
        );
        assert!(validate_regular_tree(&tree(vec![entry("x/y", "100644", "blob")])).is_err());
        assert!(validate_regular_tree(&tree(vec![entry("../escape", "100644", "blob")])).is_err());
    }
}
