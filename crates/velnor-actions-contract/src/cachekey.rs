//! Cache-key computation over semantic inputs only (cache §1).
//!
//! Keys derive from profile, config, declared env, toolchain, platform
//! (OS/arch/label/`ImageOS`/`ImageVersion`/target), format, and output
//! contract. Run IDs, absolute paths, and cwd never enter an identity.

use serde::Serialize;

use crate::canonical::{StackExtension, canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

/// Cache-key schema id, always `v1`.
pub const CACHE_SCHEMA_ID: &str = "v1";
/// Prefix shared by the MBX action namespace and task cache identity.
pub const MBX_CACHE_GENERATION_PREFIX: &str = "velnor-mbx-";
pub use crate::extension_schemas::{
    KNOWN_STACK_EXTENSION_SCHEMAS, RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA,
    is_known_stack_extension_schema,
};
/// Maximum GitHub cache-key bytes; longer keys fail generation.
pub const MAX_CACHE_KEY_BYTES: usize = 512;

/// Build the MBX action generation for one exact runtime version.
///
/// Callers validate the version through the pinned tool catalog or action
/// input gate before constructing this identity component.
#[must_use]
pub fn mbx_cache_generation(version: &str) -> String {
    format!("{MBX_CACHE_GENERATION_PREFIX}{version}")
}

/// The 13 allowed `miss_reason` values (cache §3).
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

/// Validate one `miss_reason` against the closed set.
/// # Errors
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

/// Reject empty, absolute, or control-char values in semantic inputs.
/// # Errors
pub fn validate_semantic_text(field: &'static str, value: &str) -> Result<(), ContractError> {
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

/// Workspace identity inputs: repo, stack, root, inventory.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInputs {
    /// Repository identity digest.
    pub repository_id: String,
    /// Registered detector ID.
    pub stack_id: String,
    /// Normalized repo-relative project root.
    pub project_root: String,
    /// Digest over the detector inventory.
    pub inventory_digest: String,
}

/// Concurrent-writer lane inputs.
#[derive(Debug, Clone, Serialize)]
pub struct LaneInputs {
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Detector-defined component identity.
    pub component_id: String,
    /// Task or build kind.
    pub task_kind: String,
    /// Task/build configuration digest.
    pub configuration: String,
    /// Distinct concurrent writer lane.
    pub writer_lane: String,
}

/// Platform identity inputs, including runner image metadata.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformInputs {
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Exact literal `runs-on` label.
    pub runs_on: String,
    /// Runner `ImageOS` value; `unknown` when unobserved (P03-4: the
    /// generator never splits label text into this field).
    pub image_os: String,
    /// Runner `ImageVersion` value; `unknown` when unobserved (P03-4).
    pub image_version: String,
    /// Execution target (`host` or triple).
    pub target: String,
}

/// Toolchain identity inputs: exact pins plus selected driver/runner.
#[derive(Debug, Clone, Serialize)]
pub struct ToolchainInputs {
    /// Sorted exact `<tool>@<version>` selectors.
    pub tools: Vec<String>,
    /// Sorted installed component names.
    pub components: Vec<String>,
    /// Selected compile driver.
    pub compile_driver: String,
    /// Selected test runner.
    pub test_runner: String,
}

/// Compiler/cache format inputs reported by the adapter.
#[derive(Debug, Clone, Serialize)]
pub struct FormatInputs {
    /// Adapter family (`cargo` or `mbx`).
    pub adapter: String,
    /// Reported object/source format.
    pub format: String,
    /// Reported cache generation.
    pub generation: String,
}

/// Compute `workspace_id` over the canonical workspace inputs.
/// # Errors
pub fn workspace_id(inputs: &WorkspaceInputs) -> Result<String, ContractError> {
    validate_digest(&inputs.repository_id)?;
    validate_digest(&inputs.inventory_digest)?;
    validate_semantic_text("stack_id", &inputs.stack_id)?;
    validate_semantic_text("project_root", &inputs.project_root)?;
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

/// Compute `lane_id`, binding task config plus the writer lane.
/// # Errors
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

/// Compute `platform_id` over OS/arch/label/image/target.
/// # Errors
pub fn platform_id(inputs: &PlatformInputs) -> Result<String, ContractError> {
    for (field, value) in [
        ("os", inputs.os.as_str()),
        ("arch", inputs.arch.as_str()),
        ("runs_on", inputs.runs_on.as_str()),
        ("image_os", inputs.image_os.as_str()),
        ("image_version", inputs.image_version.as_str()),
        ("target", inputs.target.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

/// Compute `toolchain_id`; tool files are not inputs and stay excluded.
/// # Errors
pub fn toolchain_id(inputs: &ToolchainInputs) -> Result<String, ContractError> {
    if inputs.tools.is_empty() {
        return Err(ContractError::identity("toolchain", "no_pinned_tools"));
    }
    let mut tools = inputs.tools.clone();
    tools.sort();
    if inputs.tools != tools {
        return Err(ContractError::identity("toolchain", "tools_must_be_sorted"));
    }
    for tool in &inputs.tools {
        validate_semantic_text("toolchain.tool", tool)?;
    }
    validate_semantic_text("compile_driver", &inputs.compile_driver)?;
    validate_semantic_text("test_runner", &inputs.test_runner)?;
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

/// Compute `cache_format_id`; unreportable MBX formats fail, never guess.
/// # Errors
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

/// Compute `stack_extension_id` over the canonical adapter extension.
/// # Errors
pub fn stack_extension_id(extension: &StackExtension) -> Result<String, ContractError> {
    if !is_known_stack_extension_schema(extension.schema.trim()) {
        return Err(ContractError::identity("stack_extension", "unknown_schema"));
    }
    Ok(digest_b3(&canonical_json_bytes(extension)?))
}

/// The exact 12-field cache identity (cache §1).
#[derive(Debug, Clone, Serialize)]
pub struct CacheIdentity {
    /// Schema id, always `v1`.
    pub schema_id: String,
    /// Registered detector ID.
    pub stack_id: String,
    /// Repository identity digest.
    pub repository_id: String,
    /// Normalized repo-relative project root.
    pub project_root: String,
    /// Detector-defined component identity.
    pub component_id: String,
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Lane identity digest.
    pub lane_id: String,
    /// Platform identity digest.
    pub platform_id: String,
    /// Toolchain identity digest.
    pub toolchain_id: String,
    /// Cache-format identity digest.
    pub cache_format_id: String,
    /// Stack-extension identity digest.
    pub stack_extension_id: String,
    /// Task input digest.
    pub input_digest: String,
}

impl CacheIdentity {
    /// Validate schema, task grammar inputs, and every digest shape.
    /// # Errors
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

/// Build `velnor-v1-<layer>-<trust>-<compat>-<snapshot>` (≤512 bytes).
/// # Errors
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

/// Build a same-compat restore prefix (snapshot omitted).
/// # Errors
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Workspace inputs with fixed digests for key tests.
    fn workspace() -> WorkspaceInputs {
        WorkspaceInputs {
            repository_id: digest_b3(b"repo"),
            stack_id: "rust".to_owned(),
            project_root: "crates/demo".to_owned(),
            inventory_digest: digest_b3(b"inventory"),
        }
    }

    #[test]
    fn keys_are_semantic_and_bounded() {
        let workspace = workspace_id(&workspace()).expect("workspace");
        let lane = lane_id(&LaneInputs {
            workspace_id: workspace,
            component_id: "pkg".to_owned(),
            task_kind: "clippy".to_owned(),
            configuration: "default".to_owned(),
            writer_lane: "lane-0".to_owned(),
        })
        .expect("lane");
        assert!(lane.starts_with("b3-"));
        let compat = digest_b3(b"compat");
        let snapshot = digest_b3(b"snapshot");
        let key = cache_key("task", "trusted", &compat, &snapshot).expect("key");
        assert!(key.len() <= MAX_CACHE_KEY_BYTES);
        assert!(key.starts_with("velnor-v1-task-trusted-"));
        assert!(
            restore_prefix("task", "trusted", &compat)
                .expect("prefix")
                .ends_with('-')
        );
        assert!(cache_key("mbx", "pr", "nope", &snapshot).is_err());
        assert!(
            lane_id(&LaneInputs {
                workspace_id: digest_b3(b"w"),
                component_id: "/abs/path".to_owned(),
                task_kind: "clippy".to_owned(),
                configuration: "default".to_owned(),
                writer_lane: "lane-0".to_owned(),
            })
            .is_err()
        );
        assert!(
            platform_id(&PlatformInputs {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                image_os: "ubuntu26".to_owned(),
                image_version: "20260928.1.0".to_owned(),
                target: "host".to_owned(),
            })
            .is_ok()
        );
        assert!(
            cache_format_id(&FormatInputs {
                adapter: "mbx".to_owned(),
                format: String::new(),
                generation: "7".to_owned(),
            })
            .is_err()
        );
        assert!(validate_miss_reason("no_entry").is_ok());
        assert!(validate_miss_reason("sometimes").is_err());
    }
}
