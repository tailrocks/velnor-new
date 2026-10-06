//! Reviewed source staging authority; never a qualified distribution record.

use super::source_build_bootstrap::{self, SourceBuildBootstrapHost, SourceBuildBootstrapTool};
use crate::MiseError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Approved source identity; no dispatch input can replace these literals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedOwnedSource {
    /// Closed owned build recipe name.
    pub tool: String,
    /// Exact reported owned release version.
    pub version: String,
    /// Owned tool source commit, distinct from generator workflow source.
    pub source_commit: String,
    /// Complete source tree.
    pub source_tree: String,
    /// Exact reviewed upstream base.
    pub upstream_base_commit: String,
    /// Immutable source tar URL.
    pub archive_url: String,
    /// Measured source tar SHA256.
    pub archive_sha256: String,
    /// Immutable receipt URL.
    pub receipt_url: String,
    /// Measured receipt SHA256.
    pub receipt_sha256: String,
    /// Immutable full-tree base patch URL.
    pub patch_url: String,
    /// Measured base patch SHA256.
    pub patch_sha256: String,
    /// Measured committed dependency lock SHA256.
    pub lockfile_sha256: String,
    /// Committed license paths and measured SHA256 values.
    pub license_files: BTreeMap<String, String>,
}

impl ApprovedOwnedSource {
    /// Validate reviewed source transport and literal identities.
    /// # Errors
    /// Rejects incomplete identities, transport mismatch, and open recipes.
    pub fn validate(&self) -> Result<(), MiseError> {
        let expected_base = match self.tool.as_str() {
            "mise" => source_build_bootstrap::official(
                SourceBuildBootstrapTool::Mise,
                SourceBuildBootstrapHost::LinuxAmd64,
            )
            .source_commit(),
            "mbx" => source_build_bootstrap::official(
                SourceBuildBootstrapTool::Mbx,
                SourceBuildBootstrapHost::LinuxAmd64,
            )
            .source_commit(),
            _ => "",
        };
        let source_base = self.archive_url.strip_suffix("/source.tar");
        let safe_base = source_base.is_some_and(|base| {
            base.strip_prefix("https://github.com/tailrocks/velnor-new/releases/download/")
                .is_some_and(|tag| {
                    !tag.is_empty()
                        && tag
                            .as_bytes()
                            .first()
                            .is_some_and(u8::is_ascii_alphanumeric)
                        && tag != "latest"
                        && !tag.contains("..")
                        && tag.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                })
        });
        let commits = [
            &self.source_commit,
            &self.source_tree,
            &self.upstream_base_commit,
        ];
        let hashes = [
            &self.archive_sha256,
            &self.receipt_sha256,
            &self.patch_sha256,
            &self.lockfile_sha256,
        ];
        if !matches!(self.tool.as_str(), "mise" | "mbx")
            || !valid_owned_version(&self.version)
            || self.version.contains("DEBUG")
            || !safe_base
            || source_base.is_none_or(|base| {
                self.receipt_url != format!("{base}/source-receipt.json")
                    || self.patch_url != format!("{base}/base.patch")
            })
            || commits.iter().any(|value| !valid_hash(value, 40))
            || hashes.iter().any(|value| !valid_hash(value, 64))
            || self.source_commit == self.upstream_base_commit
            || self.upstream_base_commit != expected_base
            || self.license_files.is_empty()
            || !self.license_files.contains_key("LICENSE")
            || self.license_files.iter().any(|(path, hash)| {
                !valid_hash(hash, 64)
                    || path.starts_with('/')
                    || !path.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.')
                    })
                    || path.split('/').any(|part| {
                        part.is_empty()
                            || part == "."
                            || part == ".."
                            || part.eq_ignore_ascii_case(".git")
                    })
                    || path.chars().any(char::is_control)
            })
        {
            return Err(MiseError::Contract {
                problem: "unapproved_owned_source".to_owned(),
            });
        }
        Ok(())
    }
}

fn valid_owned_version(value: &str) -> bool {
    let Some((release, suffix)) = value.split_once('-') else {
        return false;
    };
    let parts: Vec<_> = release.split('.').collect();
    let numeric = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        });
    let owned = suffix.strip_prefix("owned-").is_some_and(|suffix| {
        suffix.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
    });
    let velnor = suffix.strip_prefix("velnor.").is_some_and(|number| {
        !number.is_empty()
            && !number.starts_with('0')
            && number.bytes().all(|byte| byte.is_ascii_digit())
    });
    numeric && (owned || velnor)
}

fn valid_hash(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "catalog_owned_source_tests.rs"]
mod tests;
