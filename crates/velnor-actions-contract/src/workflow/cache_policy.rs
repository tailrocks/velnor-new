// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed, normalize_posix_path};
use crate::errors::ContractError;
use crate::secrets::is_secret_env_name;
use crate::workflow::cache_identity::{
    CompatibilityV1, GenerationId, check_schema, parse_strict_record, require_nonempty,
    require_sorted_unique,
};

pub const FORBIDDEN_UNIVERSAL_KEY_FIELDS: [&str; 6] = [
    "run_id",
    "run_attempt",
    "job_id",
    "application_commit",
    "head_sha",
    "source_sha",
];

pub const ALLOWED_UNIVERSAL_KEY_FIELDS: [&str; 6] = [
    "family",
    "group_digest",
    "compatibility_digest",
    "coverage_digest",
    "repair_revision",
    "namespace",
];

pub fn validate_universal_key_field(name: &str) -> Result<(), ContractError> {
    if name.is_empty() {
        return Err(ContractError::identity("cache_key", "empty_key_field"));
    }
    if FORBIDDEN_UNIVERSAL_KEY_FIELDS.contains(&name) {
        return Err(ContractError::identity(
            "cache_key",
            format!("volatile_key_input:{name}"),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImmutableObjectKind {
    Source,
    Tool,
    PrivateAsset,
    VerificationInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImmutableObjectV1 {
    pub object_id: String,
    pub kind: ImmutableObjectKind,
    pub approved_origin_id: String,
    pub immutable_locator: String,
    pub content_digest: Digest,
    pub audience_digest: Digest,
}

impl ImmutableObjectV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("immutable_object.object_id", &self.object_id)?;
        require_nonempty(
            "immutable_object.approved_origin_id",
            &self.approved_origin_id,
        )?;
        require_nonempty(
            "immutable_object.immutable_locator",
            &self.immutable_locator,
        )?;
        if self.immutable_locator.chars().any(char::is_whitespace) {
            return Err(ContractError::identity(
                "immutable_object.immutable_locator",
                "locator_has_whitespace",
            ));
        }
        let locator = self.immutable_locator.as_str();
        let remote = locator.starts_with("http://") || locator.starts_with("https://");
        if remote && !locator.contains(self.content_digest.as_str()) {
            return Err(ContractError::identity(
                "immutable_object.immutable_locator",
                "mutable_url_without_digest",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeV1 {
    pub recipe_id: Digest,
    pub unit_id: String,
    pub workspace_id: String,
    pub package_id: String,
    pub command_family: String,
    pub lane_id: String,
    pub command_builder_id: String,
    pub argv: Vec<String>,
    pub cwd_relative: String,
    pub executable_identity: String,
    pub features: Vec<String>,
    pub default_features: bool,
    pub cargo_profile: String,
    pub nextest_profile: String,
    pub targets: Vec<String>,
    pub declared_nonsecret_environment: BTreeMap<String, String>,
    pub warning_policy_digest: Digest,
    pub dependency_input_digest: Digest,
    pub workspace_dependency_digest: Digest,
    pub native_input_digest: Digest,
    pub declared_external_input_digest: Digest,
    pub required_output_roles: Vec<String>,
    pub source_bound_helper_digest: Option<Digest>,
}

impl RecipeV1 {
    pub fn computed_recipe_id(&self) -> Result<Digest, ContractError> {
        let mut value = serde_json::to_value(self)
            .map_err(|err| ContractError::CanonicalJson(err.to_string()))?;
        value
            .as_object_mut()
            .ok_or_else(|| ContractError::CanonicalJson("recipe_not_object".to_owned()))?
            .remove("recipe_id");
        Ok(digest_b3_typed(&canonical_json_bytes(&value)?))
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("recipe.unit_id", &self.unit_id)?;
        require_nonempty("recipe.workspace_id", &self.workspace_id)?;
        require_nonempty("recipe.package_id", &self.package_id)?;
        require_nonempty("recipe.command_family", &self.command_family)?;
        require_nonempty("recipe.lane_id", &self.lane_id)?;
        require_nonempty("recipe.command_builder_id", &self.command_builder_id)?;
        require_nonempty("recipe.executable_identity", &self.executable_identity)?;
        require_nonempty("recipe.cargo_profile", &self.cargo_profile)?;
        require_nonempty("recipe.nextest_profile", &self.nextest_profile)?;
        if self.argv.is_empty() {
            return Err(ContractError::identity("recipe.argv", "empty_argv"));
        }
        normalize_posix_path(&self.cwd_relative)
            .map_err(|_| ContractError::identity("recipe.cwd_relative", "invalid_relative_path"))?;
        require_sorted_unique("recipe.features", &self.features)?;
        require_sorted_unique("recipe.targets", &self.targets)?;
        require_sorted_unique("recipe.required_output_roles", &self.required_output_roles)?;
        if self.required_output_roles.is_empty() {
            return Err(ContractError::identity(
                "recipe.required_output_roles",
                "empty_output_roles",
            ));
        }
        for name in self.declared_nonsecret_environment.keys() {
            if is_secret_env_name(name) {
                return Err(ContractError::identity(
                    "recipe.declared_nonsecret_environment",
                    format!("secret_env:{name}"),
                ));
            }
        }
        let computed = self.computed_recipe_id()?;
        if computed != self.recipe_id {
            return Err(ContractError::identity(
                "recipe.recipe_id",
                "recipe_id_mismatch",
            ));
        }
        Ok(())
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
pub struct RequestedCoverageV1 {
    pub schema: u32,
    pub generation: GenerationId,
    pub compatibility: CompatibilityV1,
    pub ordered_recipe_ids: Vec<String>,
    pub recipes: Vec<RecipeV1>,
    pub immutable_objects: Vec<ImmutableObjectV1>,
    pub active_variant_ids: Vec<String>,
    pub dependency_inputs_digest: Digest,
}

impl RequestedCoverageV1 {
    pub fn coverage_digest(&self) -> Result<Digest, ContractError> {
        let mut value = serde_json::to_value(self)
            .map_err(|err| ContractError::CanonicalJson(err.to_string()))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| ContractError::CanonicalJson("coverage_not_object".to_owned()))?;
        object.remove("generation");
        object.remove("compatibility");
        object.remove("ordered_recipe_ids");
        Ok(digest_b3_typed(&canonical_json_bytes(&value)?))
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "requested_coverage.schema")?;
        self.compatibility.validate()?;
        for recipe in &self.recipes {
            recipe.validate()?;
        }
        for object in &self.immutable_objects {
            object.validate()?;
        }
        let mut seen = BTreeSet::new();
        for recipe in &self.recipes {
            if !seen.insert(recipe.recipe_id.as_str()) {
                return Err(ContractError::identity(
                    "requested_coverage.recipes",
                    "duplicate_recipe_id",
                ));
            }
        }
        let mut expected: Vec<&str> = self
            .recipes
            .iter()
            .map(|recipe| recipe.recipe_id.as_str())
            .collect();
        expected.sort_unstable();
        let ordered: Vec<&str> = self.ordered_recipe_ids.iter().map(String::as_str).collect();
        if ordered != expected {
            return Err(ContractError::identity(
                "requested_coverage.ordered_recipe_ids",
                "must_match_sorted_recipe_ids",
            ));
        }
        let mut object_ids = BTreeSet::new();
        for object in &self.immutable_objects {
            if !object_ids.insert(object.object_id.as_str()) {
                return Err(ContractError::identity(
                    "requested_coverage.immutable_objects",
                    "duplicate_object_id",
                ));
            }
        }
        require_sorted_unique(
            "requested_coverage.active_variant_ids",
            &self.active_variant_ids,
        )?;
        if self.generation.family != self.compatibility.family {
            return Err(ContractError::identity(
                "requested_coverage.generation",
                "family_mismatch",
            ));
        }
        if self.generation.compatibility_digest != self.compatibility.digest()? {
            return Err(ContractError::identity(
                "requested_coverage.generation",
                "compatibility_digest_mismatch",
            ));
        }
        if self.generation.coverage_digest != self.coverage_digest()? {
            return Err(ContractError::identity(
                "requested_coverage.generation",
                "coverage_digest_mismatch",
            ));
        }
        Ok(())
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
pub struct CompletedCoverageV1 {
    pub schema: u32,
    pub requested_coverage_digest: Digest,
    pub compatibility_digest: Digest,
    pub completed_recipe_ids: Vec<String>,
}

impl CompletedCoverageV1 {
    pub fn completed_coverage_digest(&self) -> Result<Digest, ContractError> {
        self.validate()?;
        Ok(digest_b3_typed(&canonical_json_bytes(self)?))
    }

    pub fn covers(&self, requested: &RequestedCoverageV1) -> Result<bool, ContractError> {
        let requested_digest = digest_b3_typed(&requested.to_canonical_bytes()?);
        if requested_digest != self.requested_coverage_digest {
            return Ok(false);
        }
        let requested_ids: BTreeSet<&str> = requested
            .recipes
            .iter()
            .map(|recipe| recipe.recipe_id.as_str())
            .collect();
        Ok(self
            .completed_recipe_ids
            .iter()
            .all(|id| requested_ids.contains(id.as_str())))
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "completed_coverage.schema")?;
        if self.completed_recipe_ids.is_empty() {
            return Err(ContractError::identity(
                "completed_coverage.completed_recipe_ids",
                "empty_completed_coverage",
            ));
        }
        require_sorted_unique(
            "completed_coverage.completed_recipe_ids",
            &self.completed_recipe_ids,
        )?;
        Ok(())
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
