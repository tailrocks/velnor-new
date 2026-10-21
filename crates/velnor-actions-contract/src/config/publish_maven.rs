//! Maven bundle publication config (`[[publish.maven]]`, hard decisions 6 and 8).
//!
//! One entry publishes one Gradle root's release-asset Maven bundle from a
//! release tag `<id>-v<X.Y.Z>`. The root's own `:mavenBundle` task builds
//! `<id>-<version>-maven.zip` plus its `.sha256` under `build/maven-bundle/`;
//! the zip is attested and uploaded by jobs that check out and execute no
//! repository code. The version source is the root's `gradle.properties`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::publish::{MAX_BUNDLES, is_artifact_id, validate_repo_path};
use crate::errors::ContractError;

/// Gradle task that builds the bundle; fixed, the root's convention plugin owns it.
pub const MAVEN_BUNDLE_TASK: &str = ":mavenBundle";
/// Directory (relative to the Gradle root) the task writes the zip and checksum into.
pub const MAVEN_BUNDLE_OUTPUT_DIR: &str = "build/maven-bundle";
/// Version manifest of a Gradle root (relative to the root).
pub const MAVEN_VERSION_FILE: &str = "gradle.properties";

/// One published Maven bundle (`[[publish.maven]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishMaven {
    /// Artifact id, also the tag prefix (`<id>-v<X.Y.Z>`) and the zip name stem.
    pub id: String,
    /// `[stacks.gradle]` root whose `:mavenBundle` task builds the bundle (`.` is the repository root).
    pub root: String,
    /// Enable artifact attestation by default.
    /// Set `false` only when the repository GitHub plan does not support artifact attestations.
    /// Supply `attest_reason` when this value is `false`.
    /// The generated job prints the reason as a workflow warning instead of creating an attestation.
    #[serde(default = "default_attest")]
    pub attest: bool,
    /// Give the reason when `attest = false`. Do not set it when attestation is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attest_reason: Option<String>,
}

const fn default_attest() -> bool {
    true
}

/// Longest accepted `attest_reason`.
const MAX_ATTEST_REASON: usize = 200;

fn is_clean_reason(reason: &str) -> bool {
    !reason.trim().is_empty()
        && reason.len() <= MAX_ATTEST_REASON
        && reason
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .,;:()/_-".contains(&b))
}

impl PublishMaven {
    /// Repository-relative path of the root's `gradle.properties`, the version source.
    #[must_use]
    pub fn version_file(&self) -> String {
        join_root(&self.root, MAVEN_VERSION_FILE)
    }

    /// Repository-relative directory the task writes its output into.
    #[must_use]
    pub fn output_dir(&self) -> String {
        join_root(&self.root, MAVEN_BUNDLE_OUTPUT_DIR)
    }

    /// Validate every field's grammar.
    ///
    /// # Errors
    ///
    /// Returns a config error naming the first violated key.
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

/// Validate every entry: grammar, sorted unique ids, no id already taken.
///
/// # Errors
///
/// Returns a config error naming the first violated key.
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

#[cfg(test)]
#[path = "publish_maven_tests.rs"]
mod tests;
