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
use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES as MAX_MANIFEST_BYTES;

pub(in crate::package_release_publish) struct SourceFiles<'a> {
    api: &'a GitHubApi,
    root_tree: String,
    trees: BTreeMap<String, Tree>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::package_release_publish) struct SourceFileEntry {
    pub path: String,
    pub mode: String,
    pub sha: String,
}

const MAX_SOURCE_TREE_ENTRIES: usize = 100_000;
const MAX_SOURCE_TREE_JSON_BYTES: usize = 8 * 1024 * 1024;
type SourceTreeInventory = (
    Vec<SourceFileEntry>,
    Vec<SourceFileEntry>,
    Vec<SourceFileEntry>,
);

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
    pub(in crate::package_release_publish) fn new(
        api: &'a GitHubApi,
        source_sha: &str,
    ) -> Result<Self, OrchestratorError> {
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
        self.read_bounded(path, MAX_MANIFEST_BYTES)
    }

    pub(in crate::package_release_publish) fn read_bounded(
        &mut self,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.read_bounded_if_present(path, max_bytes)?
            .ok_or_else(|| internal("release_version_manifest_missing"))
    }

    pub(in crate::package_release_publish) fn read_bounded_if_present(
        &mut self,
        path: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, OrchestratorError> {
        if max_bytes == 0 || max_bytes > MAX_MANIFEST_BYTES {
            return Err(internal("release_version_manifest_size_bound"));
        }
        let Some(entry) = self.regular_entry_if_present(path)? else {
            return Ok(None);
        };
        self.blob(&entry.sha, max_bytes).map(Some)
    }

    pub(super) fn identity(&mut self, path: &str) -> Result<SourceFileEntry, OrchestratorError> {
        let entry = self.regular_entry(path)?;
        Ok(SourceFileEntry {
            path: path.to_owned(),
            mode: entry.mode,
            sha: entry.sha,
        })
    }

    pub(in crate::package_release_publish) fn regular_files(
        &self,
    ) -> Result<Vec<SourceFileEntry>, OrchestratorError> {
        self.complete_inventory().map(|(regular, _, _)| regular)
    }

    pub(in crate::package_release_publish) fn nonregular_entries(
        &self,
    ) -> Result<Vec<SourceFileEntry>, OrchestratorError> {
        self.complete_inventory()
            .map(|(_, nonregular, _)| nonregular)
    }

    pub(in crate::package_release_publish) fn directory_entries(
        &self,
    ) -> Result<Vec<SourceFileEntry>, OrchestratorError> {
        self.complete_inventory()
            .map(|(_, _, directories)| directories)
    }

    fn complete_inventory(&self) -> Result<SourceTreeInventory, OrchestratorError> {
        let tree: Tree = self.api.get_json_bounded(
            &self
                .api
                .route(&format!("git/trees/{}?recursive=1", self.root_tree)),
            MAX_SOURCE_TREE_JSON_BYTES,
        )?;
        if tree.sha != self.root_tree
            || tree.truncated
            || tree.entries.len() > MAX_SOURCE_TREE_ENTRIES
        {
            return Err(internal("release_source_tree_identity"));
        }
        let mut entries = BTreeMap::new();
        let mut nonregular = Vec::new();
        let mut directories = Vec::new();
        let mut paths = BTreeSet::new();
        for entry in tree.entries {
            if entry.path.is_empty()
                || entry.path.len() > 1024
                || entry.path.starts_with('/')
                || entry.path.contains('\\')
                || entry
                    .path
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
                || !valid_sha(&entry.sha)
            {
                return Err(internal("release_source_tree_entry"));
            }
            if !paths.insert(entry.path.clone()) {
                return Err(internal("release_source_tree_entry"));
            }
            if entry.path == ".velnor/config.toml"
                && (entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755"))
            {
                return Err(internal("release_source_config_not_regular"));
            }
            if matches!(entry.mode.as_str(), "100644" | "100755") {
                if entry.kind != "blob" {
                    return Err(internal("release_source_tree_entry"));
                }
                entries.insert(
                    entry.path.clone(),
                    SourceFileEntry {
                        path: entry.path,
                        mode: entry.mode,
                        sha: entry.sha,
                    },
                );
            } else if entry.mode == "040000" && entry.kind == "tree" {
                directories.push(SourceFileEntry {
                    path: entry.path,
                    mode: entry.mode,
                    sha: entry.sha,
                });
            } else {
                nonregular.push(SourceFileEntry {
                    path: entry.path,
                    mode: entry.mode,
                    sha: entry.sha,
                });
            }
        }
        Ok((entries.into_values().collect(), nonregular, directories))
    }

    pub(in crate::package_release_publish) fn root_tree_sha(&self) -> &str {
        &self.root_tree
    }

    fn regular_entry(&mut self, path: &str) -> Result<Entry, OrchestratorError> {
        self.regular_entry_if_present(path)?
            .ok_or_else(|| internal("release_version_manifest_missing"))
    }

    fn regular_entry_if_present(&mut self, path: &str) -> Result<Option<Entry>, OrchestratorError> {
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
            let Some(entry) = self.entry_if_present(&tree_sha, component)? else {
                return Ok(None);
            };
            if index + 1 == components.len() {
                if entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
                    return Err(internal("release_version_manifest_not_regular"));
                }
                return Ok(Some(entry));
            }
            if entry.kind != "tree" || entry.mode != "040000" {
                return Err(internal("release_version_manifest_parent_not_tree"));
            }
            tree_sha = entry.sha;
        }
        Ok(None)
    }

    fn entry_if_present(
        &mut self,
        tree_sha: &str,
        component: &str,
    ) -> Result<Option<Entry>, OrchestratorError> {
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
        if matches.is_empty() {
            return Ok(None);
        }
        if matches.len() != 1 || !valid_sha(&matches[0].sha) {
            return Err(internal("release_version_tree_entry"));
        }
        Ok(Some(matches[0].clone()))
    }

    fn blob(&self, sha: &str, max_bytes: usize) -> Result<Vec<u8>, OrchestratorError> {
        let blob: Blob = self.api.get_json_bounded(
            &self.api.route(&format!("git/blobs/{sha}")),
            max_bytes * 2 + 1024,
        )?;
        if blob.sha != sha
            || blob.encoding != "base64"
            || blob.size == 0
            || blob.size > max_bytes
            || blob.content.len() > max_bytes * 2
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

pub(crate) fn git_blob_sha1(bytes: &[u8]) -> Result<String, OrchestratorError> {
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
