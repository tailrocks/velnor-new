use serde::{Deserialize, Serialize};

use super::bun::{is_valid_member_name, is_valid_script_name};
use super::publish::{is_artifact_id, validate_repo_path};
use crate::errors::ContractError;

pub const NPM_PUBLISH_REGISTRY_URL: &str = "https://npm.pkg.github.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpmRegistry {
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishNpm {
    pub id: String,

    pub root: String,

    pub member: String,

    pub pack_script: String,

    pub tarballs: String,

    pub registry: NpmRegistry,
}

impl PublishNpm {
    #[must_use]
    pub fn tarball_name(&self, version: &str) -> String {
        let stem = self.member.trim_start_matches('@').replace('/', "-");
        format!("{stem}-{version}.tgz")
    }

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
