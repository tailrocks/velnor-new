//! Canonical JSON bytes and BLAKE3 digest helpers.
//!
//! Canonical bytes are UTF-8 JSON with lexicographically sorted object keys,
//! declared array order, no insignificant whitespace, `/`-normalized paths,
//! and no non-finite numbers. Digests are BLAKE3 over those bytes, encoded
//! as `b3-` plus 64 lowercase hex characters.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::errors::ContractError;
use crate::vcs::VcsInputs;

mod json;
mod streaming;
mod task_identity_paths;
pub use streaming::Blake3Accumulator;

/// A validated `b3-<64 lowercase hex>` digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Digest(String);

impl Digest {
    /// Parse and validate a digest string.
    /// # Errors
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        validate_digest(value)?;
        Ok(Self(value.to_owned()))
    }

    /// Borrow the digest string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// First 16 hex chars without the `b3-` prefix.
    #[must_use]
    pub fn prefix16(&self) -> &str {
        &self.0[3..19]
    }
}

impl TryFrom<String> for Digest {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

/// A validated repository-relative POSIX path (no traversal, `/` separators).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct PosixPath(String);

impl PosixPath {
    /// Parse and normalize (`\` becomes `/`).
    /// # Errors
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        Ok(Self(normalize_posix_path(value)?))
    }

    /// Borrow the normalized path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PosixPath {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

/// Compute the typed `b3-<hex>` digest over raw bytes.
#[must_use]
pub fn digest_b3_typed(bytes: &[u8]) -> Digest {
    Digest(format!("b3-{}", blake3::hash(bytes).to_hex()))
}

/// Compute `b3-<hex>` over raw bytes.
#[must_use]
pub fn digest_b3(bytes: &[u8]) -> String {
    digest_b3_typed(bytes).as_str().to_owned()
}

/// Serialize a value to canonical JSON bytes.
/// # Errors
pub fn canonical_json_bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, ContractError> {
    let json =
        serde_json::to_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))?;
    let normalized = normalize_value(&json)?;
    let mut out = Vec::new();
    write_canonical(&normalized, &mut out);
    Ok(out)
}

/// Serialize a value to a canonical JSON string.
/// # Errors
pub fn canonical_json_str<T: Serialize + ?Sized>(value: &T) -> Result<String, ContractError> {
    let bytes = canonical_json_bytes(value)?;
    String::from_utf8(bytes).map_err(|err| ContractError::CanonicalJson(err.to_string()))
}

/// Validate a `b3-<64 lowercase hex>` digest string.
/// # Errors
pub fn validate_digest(value: &str) -> Result<(), ContractError> {
    is_valid_digest(value)
        .then_some(())
        .ok_or_else(|| ContractError::identity("digest", "malformed_b3_digest"))
}

/// Check digest shape without allocating an error.
#[must_use]
pub fn is_valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("b3-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Normalize a repository path to POSIX form (`/` separators).
/// # Errors
pub fn normalize_posix_path(path: &str) -> Result<String, ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity("path", "empty_path"));
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(ContractError::identity("path", "absolute_path"));
    }
    if normalized.split('/').any(|seg| seg == "..") {
        return Err(ContractError::identity("path", "parent_traversal"));
    }
    Ok(normalized)
}

/// Stack-neutral task identity envelope (cache §1).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskIdentity {
    /// Identity schema version; must be 1.
    pub schema_version: u32,
    /// Registered detector ID.
    pub stack_id: String,
    /// Normalized repo-relative detected project root.
    pub project_root: String,
    /// Detector-defined stable component identity.
    pub component_id: String,
    /// Task kind.
    pub task_kind: String,
    /// Stable internal task ID.
    pub task_id: String,
    /// Fixed argument vector.
    pub argv: Vec<String>,
    /// Repository-relative working directory.
    pub working_dir: String,
    /// Task configuration.
    pub configuration: TaskConfiguration,
    /// Declared input paths with content digests.
    pub inputs: Vec<TaskInput>,
    /// Upstream task IDs this task depends on (sorted, cache §1).
    pub dependencies: Vec<String>,
    /// Observed VCS revision inputs (par §4.2).
    pub vcs: VcsInputs,
    /// Toolchain identity digest.
    pub toolchain_id: String,
    /// Platform identity digest.
    pub platform_id: String,
    /// Explicit environment identities.
    pub environment: BTreeMap<String, String>,
    /// Output contract name.
    pub output_contract: String,
    /// Generator identity.
    pub generator: TaskGenerator,
    /// Typed, versioned adapter extension.
    pub stack_extension: StackExtension,
}

/// Task configuration block of [`TaskIdentity`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskConfiguration {
    /// Execution target (`host` or triple).
    pub target: String,
    /// Build profile.
    pub profile: String,
    /// Enabled features (sorted).
    pub features: Vec<String>,
    /// Extra fixed flags (sorted).
    pub flags: Vec<String>,
    /// Task contract name.
    pub task_contract: String,
    /// Selected compile driver.
    pub compile_driver: String,
    /// Selected test runner.
    pub test_runner: String,
}

/// One declared task input.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskInput {
    /// Repository-relative input path.
    pub path: String,
    /// Content digest (`b3-` + hex).
    pub digest: String,
}

/// Generator identity block of [`TaskIdentity`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskGenerator {
    /// Exact generator version.
    pub version: String,
    /// Generator target triple.
    pub target: String,
}

/// Typed, versioned stack-extension envelope.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StackExtension {
    /// Extension schema ID (unknown schemas disable reuse).
    pub schema: String,
    /// Canonical adapter data (opaque to the orchestrator).
    pub data: serde_json::Value,
}

impl TaskIdentity {
    /// Parse identity JSON, rejecting duplicate keys (cache §1).
    /// # Errors
    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let value = crate::strict_json::parse_strict_json(text)?;
        serde_json::from_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))
    }

    /// Validate relative paths, task ID, and digest shapes.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 {
            return Err(ContractError::UnsupportedSchema {
                field: "schema_version",
                found: self.schema_version.to_string(),
                expected: "1",
            });
        }
        crate::ids::validate_task_id(&self.task_id)
            .map_err(|_| ContractError::identity("task_id", "bad_task_id"))?;
        for field in [&self.project_root, &self.working_dir, &self.component_id] {
            normalize_posix_path(field)?;
        }
        for input in &self.inputs {
            normalize_posix_path(&input.path)?;
            validate_digest(&input.digest)?;
        }
        if self.argv.iter().any(|arg| arg.starts_with('/')) {
            return Err(ContractError::identity("argv", "absolute_path"));
        }
        self.validate_dependencies()?;
        self.vcs.validate()?;
        for name in self.environment.keys() {
            if crate::secrets::is_secret_env_name(name) {
                return Err(ContractError::identity(
                    "environment",
                    format!("secret_env:{name}"),
                ));
            }
        }
        Ok(())
    }

    /// Validate dependency task IDs are well-formed and sorted.
    fn validate_dependencies(&self) -> Result<(), ContractError> {
        for dep in &self.dependencies {
            crate::ids::validate_task_id(dep)?;
        }
        if self.dependencies.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(ContractError::identity("dependencies", "must_be_sorted"));
        }
        Ok(())
    }
}

/// Compute `input_digest` as BLAKE3 over canonical [`TaskIdentity`] bytes.
/// # Errors
pub fn input_digest(identity: &TaskIdentity) -> Result<String, ContractError> {
    identity.validate()?;
    let normalized = task_identity_paths::normalized(identity)?;
    Ok(digest_b3(&canonical_json_bytes(&normalized)?))
}

/// Inputs for the cache `compatibility_id` digest.
#[derive(Debug, Clone, Serialize)]
pub struct CompatibilityInputs {
    /// Schema id, always `v1`.
    pub schema_id: String,
    /// Repository identity digest.
    pub repository_id: String,
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
}

/// Compute `compatibility_id` as BLAKE3 over the canonical inputs object.
/// # Errors
pub fn compatibility_id(inputs: &CompatibilityInputs) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

/// Reject non-finite numbers and sort object keys recursively.
fn normalize_value(value: &serde_json::Value) -> Result<serde_json::Value, ContractError> {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = BTreeMap::new();
            for (key, val) in map {
                sorted.insert(key.clone(), normalize_value(val)?);
            }
            Ok(serde_json::Value::Object(sorted.into_iter().collect()))
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(normalize_value(item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        serde_json::Value::Number(num) => {
            if num.as_f64().is_some_and(f64::is_finite) || num.is_i64() || num.is_u64() {
                Ok(value.clone())
            } else {
                Err(ContractError::CanonicalJson("non_finite_number".to_owned()))
            }
        }
        other => Ok(other.clone()),
    }
}

/// Write canonical JSON: sorted keys (via [`normalize_value`]), no whitespace.
fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => out.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => out.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => out.extend_from_slice(b"false"),
        serde_json::Value::Number(num) => out.extend_from_slice(num.to_string().as_bytes()),
        serde_json::Value::String(text) => json::write_quoted(text, out),
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        serde_json::Value::Object(map) => {
            // Keys arrive pre-sorted from normalize_value's BTreeMap round-trip.
            let sorted: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            out.push(b'{');
            for (index, (key, val)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                json::write_quoted(key, out);
                out.push(b':');
                write_canonical(val, out);
            }
            out.push(b'}');
        }
    }
}
