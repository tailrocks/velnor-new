//! Exact-commit regular Git blob reads for native version-policy admission.
//!
//! The Contents API can follow symlinks. This reader walks immutable Git trees
//! instead, admits only regular-file modes and never checks out or executes code.

const HEX: &[u8; 16] = b"0123456789abcdef";

use super::super::{github::GitHubApi, identity::is_git_sha};
use crate::{OrchestratorError, internal::internal};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use sha1_checked::{Digest as _, Sha1};
use std::collections::BTreeMap;

use velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES as MAX_MANIFEST_BYTES;

pub(super) struct SourceFiles<'a> {
    api: &'a GitHubApi,
    root_tree: String,
    trees: BTreeMap<String, Tree>,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
    tree: Object,
}
#[derive(Deserialize)]
struct Object {
    sha: String,
}
#[derive(Clone, Deserialize)]
struct Tree {
    sha: String,
    truncated: bool,
    #[serde(rename = "tree")]
    entries: Vec<Entry>,
}
#[derive(Clone, Deserialize)]
struct Entry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}
#[derive(Deserialize)]
struct Blob {
    sha: String,
    size: usize,
    encoding: String,
    content: String,
}

impl<'a> SourceFiles<'a> {
    pub(super) fn new(api: &'a GitHubApi, source_sha: &str) -> Result<Self, OrchestratorError> {
        if !valid_sha(source_sha) {
            return Err(internal("release_version_source_sha"));
        }
        let commit: Commit = api.get_json(&api.route(&format!("git/commits/{source_sha}")))?;
        if commit.sha != source_sha || !valid_sha(&commit.tree.sha) {
            return Err(internal("release_version_source_commit"));
        }
        Ok(Self {
            api,
            root_tree: commit.tree.sha,
            trees: BTreeMap::new(),
        })
    }

    pub(super) fn read(&mut self, path: &str) -> Result<Vec<u8>, OrchestratorError> {
        let components: Vec<_> = path.split('/').collect();
        if path.len() > 1024
            || components.len() > 32
            || components.is_empty()
            || components.iter().any(|part| {
                part.is_empty()
                    || matches!(*part, "." | "..")
                    || part.len() > 255
                    || part.contains('\\')
            })
        {
            return Err(internal("release_version_manifest_path"));
        }
        let mut tree_sha = self.root_tree.clone();
        for (index, component) in components.iter().enumerate() {
            let entry = self.entry(&tree_sha, component)?;
            if index + 1 == components.len() {
                if entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
                    return Err(internal("release_version_manifest_not_regular"));
                }
                return self.blob(&entry.sha);
            }
            if entry.kind != "tree" || entry.mode != "040000" {
                return Err(internal("release_version_manifest_parent_not_tree"));
            }
            tree_sha = entry.sha;
        }
        Err(internal("release_version_manifest_missing"))
    }

    fn entry(&mut self, tree_sha: &str, component: &str) -> Result<Entry, OrchestratorError> {
        if !self.trees.contains_key(tree_sha) {
            let tree: Tree = self
                .api
                .get_json(&self.api.route(&format!("git/trees/{tree_sha}")))?;
            if tree.sha != tree_sha || tree.truncated {
                return Err(internal("release_version_tree_identity"));
            }
            self.trees.insert(tree_sha.to_owned(), tree);
        }
        let matches: Vec<_> = self.trees[tree_sha]
            .entries
            .iter()
            .filter(|entry| entry.path == component)
            .collect();
        if matches.len() != 1 || !valid_sha(&matches[0].sha) {
            return Err(internal("release_version_tree_entry"));
        }
        Ok(matches[0].clone())
    }

    fn blob(&self, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        let blob: Blob = self.api.get_json_bounded(
            &self.api.route(&format!("git/blobs/{sha}")),
            MAX_MANIFEST_BYTES * 2 + 1024,
        )?;
        if blob.sha != sha
            || blob.encoding != "base64"
            || blob.size == 0
            || blob.size > MAX_MANIFEST_BYTES
            || blob.content.len() > MAX_MANIFEST_BYTES * 2
        {
            return Err(internal("release_version_blob_identity"));
        }
        let bytes = STANDARD
            .decode(blob.content.replace('\n', ""))
            .map_err(|_| internal("release_version_blob_encoding"))?;
        if bytes.len() != blob.size {
            return Err(internal("release_version_blob_size"));
        }
        if git_blob_sha1(&bytes)? != sha {
            return Err(internal("release_version_blob_object_digest"));
        }
        Ok(bytes)
    }
}

fn git_blob_sha1(bytes: &[u8]) -> Result<String, OrchestratorError> {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()).as_bytes());
    hasher.update(bytes);
    let result = hasher.try_finalize();
    if result.has_collision() {
        return Err(internal("release_version_blob_collision"));
    }
    let mut digest = String::with_capacity(40);
    for byte in result.hash() {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(digest)
}

fn valid_sha(sha: &str) -> bool {
    is_git_sha(sha) && !sha.bytes().any(|byte| byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "package_release_source_files_tests.rs"]
mod tests;
