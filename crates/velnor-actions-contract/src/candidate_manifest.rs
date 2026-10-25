use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest_checks::{check_commit, check_schema, check_sha256, check_target};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateArtifactManifest {
    pub schema: u32,

    pub commit: String,

    pub target: String,

    pub toolchain: String,

    pub sha256: String,
}

impl CandidateArtifactManifest {
    pub const SCHEMA: u32 = 1;

    #[must_use]
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        check_commit(&self.commit, file, "commit")?;
        check_target(&self.target, file, "target")?;
        if !crate::targets::is_supported_target(&self.target) {
            return Err(ContractError::config(
                file,
                "target",
                format!("unsupported_target:{}", self.target),
            ));
        }
        if self.toolchain.trim().is_empty() {
            return Err(ContractError::config(file, "toolchain", "empty_toolchain"));
        }
        check_sha256(&self.sha256, file, "sha256")?;
        Ok(())
    }
}

pub fn require_release_version(version: &str, file: &str) -> Result<(), ContractError> {
    if is_release_version(version) {
        Ok(())
    } else {
        Err(ContractError::config(file, "version", "non_release_build"))
    }
}

fn is_release_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}
