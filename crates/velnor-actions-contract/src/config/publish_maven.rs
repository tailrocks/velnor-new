// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::publish::{MAX_BUNDLES, is_artifact_id, validate_repo_path};
use crate::errors::ContractError;

pub const MAVEN_BUNDLE_TASK: &str = ":mavenBundle";

pub const MAVEN_BUNDLE_OUTPUT_DIR: &str = "build/maven-bundle";

pub const MAVEN_VERSION_FILE: &str = "gradle.properties";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishMaven {
    pub id: String,

    pub root: String,

    #[serde(default = "default_attest")]
    pub attest: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attest_reason: Option<String>,
}

const fn default_attest() -> bool {
    true
}

const MAX_ATTEST_REASON: usize = 200;

fn is_clean_reason(reason: &str) -> bool {
    !reason.trim().is_empty()
        && reason.len() <= MAX_ATTEST_REASON
        && reason
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .,;:()/_-".contains(&b))
}

impl PublishMaven {
    #[must_use]
    pub fn version_file(&self) -> String {
        join_root(&self.root, MAVEN_VERSION_FILE)
    }

    #[must_use]
    pub fn output_dir(&self) -> String {
        join_root(&self.root, MAVEN_BUNDLE_OUTPUT_DIR)
    }

    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        if !is_artifact_id(&self.id) {
            return Err(ContractError::config(
                file,
                format!("{key}.id"),
                format!("bad_artifact_id:{}", self.id),
            ));
        }
        match (self.attest, self.attest_reason.as_deref()) {
            (true, None) => {}
            (true, Some(_)) => {
                return Err(ContractError::config(
                    file,
                    format!("{key}.attest_reason"),
                    "only_with_attest_false",
                ));
            }
            (false, Some(reason)) if is_clean_reason(reason) => {}
            (false, _) => {
                return Err(ContractError::config(
                    file,
                    format!("{key}.attest_reason"),
                    "required_clean_text_with_attest_false",
                ));
            }
        }
        if self.root != "." {
            validate_repo_path(file, &format!("{key}.root"), &self.root)?;
        }
        Ok(())
    }
}

fn join_root(root: &str, rel: &str) -> String {
    if root == "." {
        rel.to_owned()
    } else {
        format!("{root}/{rel}")
    }
}

pub(super) fn validate_entries<'a>(
    entries: &'a [PublishMaven],
    file: &str,
    taken: &mut BTreeSet<&'a str>,
) -> Result<(), ContractError> {
    if entries.len() > MAX_BUNDLES {
        return Err(ContractError::config(file, "publish.maven", "too_many"));
    }
    for (index, entry) in entries.iter().enumerate() {
        entry.validate(file, &format!("publish.maven[{index}]"))?;
        if !taken.insert(entry.id.as_str()) {
            return Err(ContractError::config(
                file,
                "publish.maven",
                format!("duplicate_id:{}", entry.id),
            ));
        }
    }
    if entries.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(ContractError::config(
            file,
            "publish.maven",
            "must_be_sorted_by_id",
        ));
    }
    Ok(())
}
