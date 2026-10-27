// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::cachekey::MAX_CACHE_KEY_BYTES;
use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use crate::errors::ContractError;
use crate::strict_json::parse_strict_json;

pub const GC1_NAMESPACE: &str = "velnor-gc1";

pub const GC1_SCHEMA: u32 = 1;

pub fn parse_strict_record<T: DeserializeOwned>(text: &str) -> Result<T, ContractError> {
    let value = parse_strict_json(text)?;
    serde_json::from_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))
}

pub fn check_schema(schema: u32, field: &'static str) -> Result<(), ContractError> {
    if schema != GC1_SCHEMA {
        return Err(ContractError::UnsupportedSchema {
            field,
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}

pub fn require_nonempty(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::identity(field, "unknown_required_identity"));
    }
    Ok(())
}

pub fn require_sorted_unique(field: &'static str, values: &[String]) -> Result<(), ContractError> {
    for value in values {
        require_nonempty(field, value)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ContractError::identity(field, "must_be_sorted_unique"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gc1Family {
    Sources,
    Tools,
    State,
}

impl Gc1Family {
    pub const ALL: [Self; 3] = [Self::Sources, Self::Tools, Self::State];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sources => "sources",
            Self::Tools => "tools",
            Self::State => "state",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        Self::ALL
            .into_iter()
            .find(|family| family.as_str() == value)
            .ok_or_else(|| ContractError::identity("family", format!("unknown_family:{value}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityV1 {
    pub schema: u32,
    pub family: Gc1Family,
    pub repository_id: u64,
    pub workspace_id: Digest,
    pub host_os: String,
    pub host_arch: String,
    pub runner_image_identity: String,
    pub abi_identity: String,
    pub toolchain_identity: String,
    pub compiler_identity: String,
    pub native_environment_digest: Digest,
    pub target_environment_ids: Vec<String>,
    pub path_layout_digest: Digest,
    pub lane_scheme_digest: Digest,
    pub archive_format: String,
    pub observer_interface_version: u32,
    pub source_policy_digest: Option<Digest>,
    pub authorized_audience_digest: Option<Digest>,
}

impl CompatibilityV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "compatibility.schema")?;
        if self.repository_id == 0 {
            return Err(ContractError::identity(
                "compatibility.repository_id",
                "must_be_positive",
            ));
        }
        require_nonempty("compatibility.host_os", &self.host_os)?;
        require_nonempty("compatibility.host_arch", &self.host_arch)?;
        require_nonempty(
            "compatibility.runner_image_identity",
            &self.runner_image_identity,
        )?;
        require_nonempty("compatibility.abi_identity", &self.abi_identity)?;
        require_nonempty("compatibility.toolchain_identity", &self.toolchain_identity)?;
        require_nonempty("compatibility.compiler_identity", &self.compiler_identity)?;
        require_nonempty("compatibility.archive_format", &self.archive_format)?;
        require_sorted_unique(
            "compatibility.target_environment_ids",
            &self.target_environment_ids,
        )?;
        Ok(())
    }

    pub fn digest(&self) -> Result<Digest, ContractError> {
        self.validate()?;
        let mut value = serde_json::to_value(self)
            .map_err(|err| ContractError::CanonicalJson(err.to_string()))?;
        value
            .as_object_mut()
            .ok_or_else(|| ContractError::CanonicalJson("compatibility_not_object".to_owned()))?
            .remove("runner_image_identity");
        Ok(digest_b3_typed(&canonical_json_bytes(&value)?))
    }

    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let record: Self = parse_strict_record(text)?;
        record.validate()?;
        Ok(record)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationId {
    pub family: Gc1Family,
    pub group_digest: Digest,
    pub compatibility_digest: Digest,
    pub coverage_digest: Digest,
    pub repair_revision: u64,
}

impl GenerationId {
    #[must_use]
    pub fn cache_key(&self) -> String {
        format!(
            "{GC1_NAMESPACE}-{}-{}-{}-{}-r{}",
            self.family.as_str(),
            self.group_digest.as_str(),
            self.compatibility_digest.as_str(),
            self.coverage_digest.as_str(),
            self.repair_revision
        )
    }

    pub fn validate_key(key: &str) -> Result<(), ContractError> {
        Self::parse_key(key)?;
        Ok(())
    }

    pub fn parse_key(key: &str) -> Result<Self, ContractError> {
        if key.len() > MAX_CACHE_KEY_BYTES {
            return Err(ContractError::identity("cache_key", "key_too_long"));
        }
        let body = key
            .strip_prefix(&format!("{GC1_NAMESPACE}-"))
            .ok_or_else(|| ContractError::identity("cache_key", "unknown_namespace"))?;
        let (family_raw, digests) = body
            .split_once('-')
            .ok_or_else(|| ContractError::identity("cache_key", "malformed_key"))?;
        let (digest_part, repair_raw) = digests
            .rsplit_once("-r")
            .ok_or_else(|| ContractError::identity("cache_key", "malformed_key"))?;
        let repair_revision = repair_raw
            .parse::<u64>()
            .map_err(|_| ContractError::identity("cache_key", "malformed_repair"))?;
        let chunks: Vec<&str> = digest_part.split('-').collect();
        if chunks.len() != 6 {
            return Err(ContractError::identity("cache_key", "malformed_key"));
        }
        let group_digest = Digest::parse(&format!("{}-{}", chunks[0], chunks[1]))
            .map_err(|_| ContractError::identity("cache_key", "malformed_group_digest"))?;
        let compatibility_digest = Digest::parse(&format!("{}-{}", chunks[2], chunks[3]))
            .map_err(|_| ContractError::identity("cache_key", "malformed_compatibility_digest"))?;
        let coverage_digest = Digest::parse(&format!("{}-{}", chunks[4], chunks[5]))
            .map_err(|_| ContractError::identity("cache_key", "malformed_coverage_digest"))?;
        Ok(Self {
            family: Gc1Family::parse(family_raw)?,
            group_digest,
            compatibility_digest,
            coverage_digest,
            repair_revision,
        })
    }

    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        parse_strict_record(text)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        canonical_json_bytes(self)
    }
}
