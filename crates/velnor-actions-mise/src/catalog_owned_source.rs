// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::source_build_bootstrap::{self, SourceBuildBootstrapHost, SourceBuildBootstrapTool};
use crate::MiseError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedOwnedSource {
    pub tool: String,
    pub version: String,
    pub source_commit: String,
    pub source_tree: String,
    pub upstream_base_commit: String,
    pub archive_url: String,
    pub archive_sha256: String,
    pub receipt_url: String,
    pub receipt_sha256: String,
    pub patch_url: String,
    pub patch_sha256: String,
    pub lockfile_sha256: String,
    pub license_files: BTreeMap<String, String>,
}

impl ApprovedOwnedSource {
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
