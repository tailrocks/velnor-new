// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::{StackExtension, canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

#[path = "cachekey_platform.rs"]
mod platform;
pub use platform::{
    MAX_OBSERVED_PLATFORM_FACT_BYTES, PlatformInputs, observed_platform_id, platform_id,
};

pub const CACHE_SCHEMA_ID: &str = "v1";

pub const MBX_CACHE_GENERATION_PREFIX: &str = "velnor-mbx-";
pub use crate::extension_schemas::{
    KNOWN_STACK_EXTENSION_SCHEMAS, RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA,
    is_known_stack_extension_schema,
};

pub const MAX_CACHE_KEY_BYTES: usize = 512;

#[must_use]
pub fn mbx_cache_generation(version: &str) -> String {
    format!("{MBX_CACHE_GENERATION_PREFIX}{version}-share-out-dir-disabled-v1")
}

pub const MISS_REASONS: [&str; 13] = [
    "no_entry",
    "compatibility_mismatch",
    "input_digest_mismatch",
    "trust_scope_mismatch",
    "cache_unavailable",
    "cache_corrupt",
    "cache_expired",
    "cache_write_disabled",
    "task_not_eligible",
    "task_result_incomplete",
    "forced_uncached",
    "tool_missing",
    "source_missing",
];

pub const FINAL_GATE_REASONS: [&str; 3] = [
    "planned_job_skipped",
    "gate_mismatch",
    "not_affected_on_full_event",
];

pub fn validate_final_miss_reason(reason: &str) -> Result<(), ContractError> {
    if FINAL_GATE_REASONS.contains(&reason) {
        Ok(())
    } else {
        validate_miss_reason(reason)
    }
}

pub fn validate_miss_reason(reason: &str) -> Result<(), ContractError> {
    if MISS_REASONS.contains(&reason) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "miss_reason",
            format!("unknown_reason:{reason}"),
        ))
    }
}

pub(super) fn validate_semantic_text(
    field: &'static str,
    value: &str,
) -> Result<(), ContractError> {
    if value.is_empty() || value.starts_with('/') || value.contains('\\') {
        return Err(ContractError::identity(field, "non_semantic_input"));
    }
    if value
        .chars()
        .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(ContractError::identity(field, "non_semantic_input"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInputs {
    pub repository_id: String,

    pub stack_id: String,

    pub project_root: String,

    pub inventory_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaneInputs {
    pub workspace_id: String,

    pub component_id: String,

    pub task_kind: String,

    pub configuration: String,

    pub writer_lane: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainInputs {
    pub tools: Vec<String>,

    pub components: Vec<String>,

    pub compile_driver: String,

    pub test_runner: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_test_tools: Vec<SelectedTestTool>,
}

pub use crate::cachekey_selected_tools::{
    SelectedTestTool, SelectedTestToolSourceBinding, toolchain_id,
};

#[derive(Debug, Clone, Serialize)]
pub struct FormatInputs {
    pub adapter: String,

    pub format: String,

    pub generation: String,
}

pub fn workspace_id(inputs: &WorkspaceInputs) -> Result<String, ContractError> {
    validate_digest(&inputs.repository_id)?;
    validate_digest(&inputs.inventory_digest)?;
    validate_semantic_text("stack_id", &inputs.stack_id)?;
    validate_semantic_text("project_root", &inputs.project_root)?;
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

pub fn lane_id(inputs: &LaneInputs) -> Result<String, ContractError> {
    validate_digest(&inputs.workspace_id)?;
    for (field, value) in [
        ("component_id", inputs.component_id.as_str()),
        ("task_kind", inputs.task_kind.as_str()),
        ("configuration", inputs.configuration.as_str()),
        ("writer_lane", inputs.writer_lane.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

pub fn cache_format_id(inputs: &FormatInputs) -> Result<String, ContractError> {
    if inputs.adapter != "cargo" && inputs.adapter != "mbx" && inputs.adapter != "tofu" {
        return Err(ContractError::identity("cache_format", "unknown_adapter"));
    }
    if inputs.format.trim().is_empty() || inputs.generation.trim().is_empty() {
        return Err(ContractError::identity(
            "cache_format",
            "format_unreportable",
        ));
    }
    validate_semantic_text("format", &inputs.format)?;
    validate_semantic_text("generation", &inputs.generation)?;
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

pub fn stack_extension_id(extension: &StackExtension) -> Result<String, ContractError> {
    if !is_known_stack_extension_schema(extension.schema.trim()) {
        return Err(ContractError::identity("stack_extension", "unknown_schema"));
    }
    Ok(digest_b3(&canonical_json_bytes(extension)?))
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheIdentity {
    pub schema_id: String,

    pub stack_id: String,

    pub repository_id: String,

    pub project_root: String,

    pub component_id: String,

    pub workspace_id: String,

    pub lane_id: String,

    pub platform_id: String,

    pub toolchain_id: String,

    pub cache_format_id: String,

    pub stack_extension_id: String,

    pub input_digest: String,
}

impl CacheIdentity {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_id != CACHE_SCHEMA_ID {
            return Err(ContractError::identity("schema_id", "must_be_v1"));
        }
        validate_semantic_text("stack_id", &self.stack_id)?;
        validate_semantic_text("project_root", &self.project_root)?;
        validate_semantic_text("component_id", &self.component_id)?;
        for value in [
            self.repository_id.as_str(),
            self.workspace_id.as_str(),
            self.lane_id.as_str(),
            self.platform_id.as_str(),
            self.toolchain_id.as_str(),
            self.cache_format_id.as_str(),
            self.stack_extension_id.as_str(),
            self.input_digest.as_str(),
        ] {
            validate_digest(value)?;
        }
        Ok(())
    }
}

pub fn cache_key(
    layer: &str,
    trust: &str,
    compatibility: &str,
    snapshot: &str,
) -> Result<String, ContractError> {
    if !matches!(layer, "sources" | "mbx" | "task" | "tofu-providers") {
        return Err(ContractError::identity("cache.layer", "unknown_layer"));
    }
    if !matches!(trust, "trusted" | "pr") {
        return Err(ContractError::identity("cache.trust", "unknown_trust"));
    }
    validate_digest(compatibility)?;
    validate_digest(snapshot)?;
    let key = format!("velnor-v1-{layer}-{trust}-{compatibility}-{snapshot}");
    if key.len() > MAX_CACHE_KEY_BYTES {
        return Err(ContractError::identity("cache.key", "key_too_long"));
    }
    Ok(key)
}

pub fn restore_prefix(
    layer: &str,
    trust: &str,
    compatibility: &str,
) -> Result<String, ContractError> {
    if !matches!(layer, "sources" | "mbx" | "task" | "tofu-providers") {
        return Err(ContractError::identity("cache.layer", "unknown_layer"));
    }
    if !matches!(trust, "trusted" | "pr") {
        return Err(ContractError::identity("cache.trust", "unknown_trust"));
    }
    validate_digest(compatibility)?;
    Ok(format!("velnor-v1-{layer}-{trust}-{compatibility}-"))
}
