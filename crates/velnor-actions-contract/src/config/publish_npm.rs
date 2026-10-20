//! npm publication config (`[[publish.npm]]`, hard decisions 6 and 8).
//!
//! One entry publishes one workspace member to GitHub Packages from a release
//! tag `<id>-v<X.Y.Z>`. The package is packed by a declared root script of the
//! repository (twice, to prove the tarball is byte-identical), attested, and
//! pushed by a job that checks out and executes no repository code.

use serde::{Deserialize, Serialize};

use super::bun::{is_valid_member_name, is_valid_script_name};
use super::publish::{is_artifact_id, validate_repo_path};
use crate::errors::ContractError;

/// The only registry an npm entry may publish to.
pub const NPM_PUBLISH_REGISTRY_URL: &str = "https://npm.pkg.github.com";

/// Closed registry choice of an npm entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpmRegistry {
    /// GitHub Packages (`npm.pkg.github.com`).
    Github,
}

/// One published npm package (`[[publish.npm]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishNpm {
    /// Artifact id, also the tag prefix (`<id>-v<X.Y.Z>`).
    pub id: String,
    /// `[stacks.bun]` root holding the workspace (`.` is the repository root).
    pub root: String,
    /// Workspace member published at this tag (its `package.json` `name`).
    pub member: String,
    /// Root `package.json` script that writes the tarballs.
    pub pack_script: String,
    /// Repository-relative directory the script writes `<scope>-<name>-<version>.tgz` into.
    pub tarballs: String,
    /// Target registry.
    pub registry: NpmRegistry,
}

impl PublishNpm {
    /// File name of the member's tarball for `version`, as `npm pack` names it.
    #[must_use]
    pub fn tarball_name(&self, version: &str) -> String {
        let stem = self.member.trim_start_matches('@').replace('/', "-");
        format!("{stem}-{version}.tgz")
    }

    /// Validate every field's grammar.
    ///
    /// # Errors
    ///
    /// Returns a config error naming the first violated key.
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let fail = |field: &str, problem: String| {
            ContractError::config(file, format!("{key}.{field}"), problem)
        };
        if !is_artifact_id(&self.id) {
            return Err(fail("id", format!("bad_artifact_id:{}", self.id)));
        }
        if !is_valid_member_name(&self.member) || !self.member.starts_with('@') {
            return Err(fail("member", format!("bad_scoped_member:{}", self.member)));
        }
        if !is_valid_script_name(&self.pack_script) {
            return Err(fail(
                "pack_script",
                format!("bad_script:{}", self.pack_script),
            ));
        }
        if self.root != "." {
            validate_repo_path(file, &format!("{key}.root"), &self.root)?;
        }
        validate_repo_path(file, &format!("{key}.tarballs"), &self.tarballs)
    }
}

#[cfg(test)]
#[path = "publish_npm_tests.rs"]
mod tests;
