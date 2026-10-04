//! Immutable MBX export identities; descriptor shape grants no provenance authority.
use std::collections::BTreeMap;

use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Separate helper compilation from repository validation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MbxCacheDomain {
    /// Generator helper compilation, never restored by validation.
    Helper,
    /// Selected repository compilation and scheduler state.
    Validation,
}

impl MbxCacheDomain {
    /// Fixed namespace component.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Helper => "helper",
            Self::Validation => "validation",
        }
    }
}

/// Exact owner executable; qualified acquisition remains the source owner's duty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxOwnerIdentity {
    /// Exact release version, without a floating selector.
    pub version: String,
    /// SHA-256 of the executable before any invocation.
    pub binary_sha256: String,
    /// Complete immutable native distribution qualification digest.
    pub qualification_identity: String,
    /// Original owner source commit.
    pub source_sha: String,
}

impl MbxOwnerIdentity {
    /// Validate identity shape, without granting executable acquisition authority.
    /// # Errors
    /// Rejects floating versions, malformed hashes and missing qualification.
    pub fn validate(&self) -> Result<(), ContractError> {
        if !canonical_version(&self.version)
            || !crate::ids::is_lower_hex_len(&self.binary_sha256, 64)
            || !crate::ids::is_lower_hex_len(&self.source_sha, 40)
            || !crate::ids::is_lower_hex_len(&self.qualification_identity, 64)
        {
            return Err(invalid("invalid_owner_identity"));
        }
        Ok(())
    }
}

fn canonical_version(value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    let (release, build) = value
        .split_once('+')
        .map_or((value, None), |(release, build)| (release, Some(build)));
    let (core, prerelease) = release
        .split_once('-')
        .map_or((release, None), |(core, tag)| (core, Some(tag)));
    let numeric = |part: &str| {
        !part.is_empty()
            && (part.len() == 1 || !part.starts_with('0'))
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && part.parse::<u64>().is_ok()
    };
    let components = core.split('.').collect::<Vec<_>>();
    if components.len() != 3 || !components.iter().all(|part| numeric(part)) {
        return false;
    }
    let identifiers = |tag: &str, canonical_numeric: bool| {
        !tag.is_empty()
            && tag.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && (!canonical_numeric
                        || !part.bytes().all(|byte| byte.is_ascii_digit())
                        || numeric(part))
            })
    };
    prerelease.is_none_or(|tag| identifiers(tag, true))
        && build.is_none_or(|tag| identifiers(tag, false))
}

/// Complete compiled export cohort; no caller path, key or artifact override exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxExportDescriptor {
    /// Closed compatibility domain.
    pub domain: MbxCacheDomain,
    /// Exact validation or helper job producing this state.
    pub producer_job_id: String,
    /// Literal supported runner label.
    pub runs_on: String,
    /// Exact executable platform target.
    pub target: String,
    /// Sorted normalized workspace roots, relative to checkout.
    pub workspace_roots: Vec<String>,
    /// Complete compiler/features/profile/flags/layout compatibility digest.
    pub configuration_digest: String,
    /// Exact selected task identities and semantic recipe digests.
    pub task_digests: BTreeMap<String, String>,
    /// Fresh source-qualified native executable identity.
    pub owner: MbxOwnerIdentity,
    /// Exact supported MBX action source commit.
    pub action_sha: String,
}

impl MbxExportDescriptor {
    /// Validate canonical identity shape. Source owners authenticate every claim.
    /// # Errors
    /// Rejects unsupported platforms, mutable identities or foreign task metadata.
    pub fn validate(&self) -> Result<(), ContractError> {
        super::jobs::validate_job_id(&self.producer_job_id)?;
        self.owner.validate()?;
        crate::canonical::validate_digest(&self.configuration_digest)?;
        if crate::tool_target_for_runner_label(&self.runs_on) != Some(self.target.as_str())
            || match self.domain {
                MbxCacheDomain::Helper => self.producer_job_id != "plan",
                MbxCacheDomain::Validation => !self.producer_job_id.starts_with("rust-"),
            }
            || self.workspace_roots.is_empty()
            || self.workspace_roots.len() > 2048
            || self
                .workspace_roots
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.task_digests.len() > 2048
            || (self.domain == MbxCacheDomain::Validation && self.task_digests.is_empty())
            || !crate::ids::is_lower_hex_len(&self.action_sha, 40)
        {
            return Err(invalid("invalid_export_descriptor"));
        }
        for root in &self.workspace_roots {
            if root.is_empty()
                || root.len() > 4096
                || root.chars().any(char::is_control)
                || root.contains("${{")
                || root.contains('\\')
                || root.starts_with('/')
                || (root != "." && root.split('/').any(|part| matches!(part, "" | "." | "..")))
            {
                return Err(invalid("invalid_workspace_root"));
            }
        }
        for (task, digest) in &self.task_digests {
            crate::validate_task_id(task)?;
            crate::canonical::validate_digest(digest)?;
        }
        Ok(())
    }

    /// Identity includes native manifest/comparison schemas and path-layout contract.
    /// # Errors
    /// Rejects unvalidated descriptors and noncanonical serialization.
    pub fn identity(&self) -> Result<String, ContractError> {
        self.validate()?;
        Ok(crate::digest_b3(&crate::canonical::canonical_json_bytes(
            &(
                "velnor-mbx-export-v3",
                "native-manifest-v4-comparison-v3",
                "checkout-relative-target-and-build-roots",
                self,
            ),
        )?))
    }

    /// Closed cache namespace; fallback relaxes only the immutable snapshot suffix.
    /// # Errors
    pub fn cache_prefix(&self) -> Result<String, ContractError> {
        Ok(format!(
            "velnor-v3-mbx-{}-${{{{ github.repository_id }}}}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-${{{{ env.VELNOR_CACHE_IMAGE }}}}-{}-",
            self.domain.name(),
            self.identity()?
        ))
    }

    /// Exact same-run/attempt artifact name; never a wildcard.
    /// # Errors
    pub fn artifact_name(&self) -> Result<String, ContractError> {
        Ok(format!(
            "velnor-mbx-export-{}-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}",
            self.identity()?
        ))
    }

    /// Fixed native bundle path, distinct from executable and receipt roots.
    /// # Errors
    pub fn bundle_root(&self) -> Result<String, ContractError> {
        Ok(format!(
            "${{{{ runner.temp }}}}/velnor/mbx-export/{}/bundle",
            self.identity()?
        ))
    }

    /// Owner comparison state stays outside the transported directory.
    /// # Errors
    pub fn comparison_path(&self) -> Result<String, ContractError> {
        Ok(format!(
            "${{{{ runner.temp }}}}/velnor/mbx-export/{}/comparison.json",
            self.identity()?
        ))
    }

    /// All task invocations within this exact producer feed one owner export group.
    /// # Errors
    pub fn export_group(&self) -> Result<String, ContractError> {
        Ok(format!(
            "velnor-mbx-{}-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}",
            self.identity()?
        ))
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("mbx_export", reason)
}

#[cfg(test)]
#[path = "mbx_export_descriptor_tests.rs"]
mod tests;
