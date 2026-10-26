// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Serialize;

use crate::errors::ContractError;
use crate::vcs::VcsInputs;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Digest(String);

impl Digest {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        validate_digest(value)?;
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct PosixPath(String);

impl PosixPath {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        Ok(Self(normalize_posix_path(value)?))
    }

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

#[must_use]
pub fn digest_b3_typed(bytes: &[u8]) -> Digest {
    Digest(format!("b3-{}", blake3::hash(bytes).to_hex()))
}

#[must_use]
pub fn digest_b3(bytes: &[u8]) -> String {
    digest_b3_typed(bytes).as_str().to_owned()
}

pub fn canonical_json_bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, ContractError> {
    let json =
        serde_json::to_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))?;
    let mut out = Vec::with_capacity(256);
    write_canonical(&json, &mut out)?;
    Ok(out)
}

pub fn canonical_json_str<T: Serialize + ?Sized>(value: &T) -> Result<String, ContractError> {
    let bytes = canonical_json_bytes(value)?;
    String::from_utf8(bytes).map_err(|err| ContractError::CanonicalJson(err.to_string()))
}

pub fn validate_digest(value: &str) -> Result<(), ContractError> {
    is_valid_digest(value)
        .then_some(())
        .ok_or_else(|| ContractError::identity("digest", "malformed_b3_digest"))
}

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskIdentity {
    pub schema_version: u32,

    pub stack_id: String,

    pub project_root: String,

    pub component_id: String,

    pub task_kind: String,

    pub task_id: String,

    pub argv: Vec<String>,

    pub working_dir: String,

    pub configuration: TaskConfiguration,

    pub inputs: Vec<TaskInput>,

    pub dependencies: Vec<String>,

    pub vcs: VcsInputs,

    pub toolchain_id: String,

    pub platform_id: String,

    pub environment: BTreeMap<String, String>,

    pub output_contract: String,

    pub generator: TaskGenerator,

    pub stack_extension: StackExtension,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskConfiguration {
    pub target: String,

    pub profile: String,

    pub features: Vec<String>,

    pub flags: Vec<String>,

    pub task_contract: String,

    pub compile_driver: String,

    pub test_runner: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskInput {
    pub path: String,

    pub digest: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskGenerator {
    pub version: String,

    pub target: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StackExtension {
    pub schema: String,

    pub data: serde_json::Value,
}

impl TaskIdentity {
    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let value = crate::strict_json::parse_strict_json(text)?;
        serde_json::from_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))
    }

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

pub fn input_digest(identity: &TaskIdentity) -> Result<String, ContractError> {
    identity.validate()?;
    Ok(digest_b3(&canonical_json_bytes(identity)?))
}

#[derive(Debug, Clone, Serialize)]
pub struct CompatibilityInputs {
    pub schema_id: String,

    pub repository_id: String,

    pub workspace_id: String,

    pub lane_id: String,

    pub platform_id: String,

    pub toolchain_id: String,

    pub cache_format_id: String,

    pub stack_extension_id: String,
}

pub fn compatibility_id(inputs: &CompatibilityInputs) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) -> Result<(), ContractError> {
    match value {
        serde_json::Value::Null => out.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => out.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => out.extend_from_slice(b"false"),
        serde_json::Value::Number(num) => {
            if num.as_f64().is_some_and(f64::is_finite) || num.is_i64() || num.is_u64() {
                out.extend_from_slice(num.to_string().as_bytes());
            } else {
                return Err(ContractError::CanonicalJson("non_finite_number".to_owned()));
            }
        }
        serde_json::Value::String(text) => write_quoted(text, out),
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_unstable_by_key(|(left, _)| *left);
            out.push(b'{');
            for (index, (key, val)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_quoted(key, out);
                out.push(b':');
                write_canonical(val, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn write_quoted(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    if !text.bytes().any(|b| b == b'"' || b == b'\\' || b < 0x20) {
        out.extend_from_slice(text.as_bytes());
        out.push(b'"');
        return;
    }
    for ch in text.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0_u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 15)]));
    }
    encoded
}
