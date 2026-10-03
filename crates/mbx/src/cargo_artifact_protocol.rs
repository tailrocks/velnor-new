//! Cargo public protocol authority: <https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages>.
//! Environment output, rendered diagnostics and extensions stay private.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

pub const CARGO_CAPTURE_SCHEMA_VERSION: u32 = 1;
const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoTarget {
    pub name: String,
    pub kind: Vec<String>,
    pub crate_types: Vec<String>,
    pub src_path: PathBuf,
    pub edition: String,
    #[serde(flatten, skip_serializing)]
    pub additional: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoProfile {
    pub opt_level: CargoOptimization,
    pub debuginfo: CargoDebuginfo,
    pub debug_assertions: bool,
    pub overflow_checks: bool,
    pub test: bool,
    #[serde(flatten, skip_serializing)]
    pub additional: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CargoOptimization {
    #[serde(rename = "0")]
    Level0,
    #[serde(rename = "1")]
    Level1,
    #[serde(rename = "2")]
    Level2,
    #[serde(rename = "3")]
    Level3,
    #[serde(rename = "s")]
    Size,
    #[serde(rename = "z")]
    MinimumSize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CargoDebuginfo {
    Default,
    Level0,
    Level1,
    Level2,
    LineDirectivesOnly,
    LineTablesOnly,
}

impl<'de> Deserialize<'de> for CargoDebuginfo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(Self::Default),
            Value::Number(number) if number.as_u64() == Some(0) => Ok(Self::Level0),
            Value::Number(number) if number.as_u64() == Some(1) => Ok(Self::Level1),
            Value::Number(number) if number.as_u64() == Some(2) => Ok(Self::Level2),
            Value::String(label) if label == "line-directives-only" => Ok(Self::LineDirectivesOnly),
            Value::String(label) if label == "line-tables-only" => Ok(Self::LineTablesOnly),
            _ => Err(serde::de::Error::custom(
                "unsupported public Cargo debuginfo value",
            )),
        }
    }
}

impl Serialize for CargoDebuginfo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Default => serializer.serialize_none(),
            Self::Level0 => serializer.serialize_u8(0),
            Self::Level1 => serializer.serialize_u8(1),
            Self::Level2 => serializer.serialize_u8(2),
            Self::LineDirectivesOnly => serializer.serialize_str("line-directives-only"),
            Self::LineTablesOnly => serializer.serialize_str("line-tables-only"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoArtifactObservation {
    pub package_id: String,
    pub manifest_path: PathBuf,
    pub target: CargoTarget,
    pub profile: CargoProfile,
    pub features: Vec<String>,
    pub filenames: Vec<PathBuf>,
    pub executable: Option<PathBuf>,
    pub fresh: bool,
    #[serde(flatten, skip_serializing)]
    pub additional: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoBuildScriptObservation {
    pub package_id: String,
    pub linked_libs: Vec<String>,
    pub linked_paths: Vec<String>,
    pub cfgs: Vec<String>,
    #[serde(skip_serializing)]
    pub env: Vec<(String, String)>,
    pub out_dir: PathBuf,
    #[serde(flatten, skip_serializing)]
    pub additional: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason")]
pub enum CargoMessage {
    #[serde(rename = "compiler-artifact")]
    CompilerArtifact(CargoArtifactObservation),
    #[serde(rename = "build-script-executed")]
    BuildScriptExecuted(CargoBuildScriptObservation),
    #[serde(rename = "compiler-message")]
    CompilerMessage {
        package_id: String,
        manifest_path: PathBuf,
        target: CargoTarget,
        #[serde(skip_serializing)]
        message: Value,
        #[serde(flatten, skip_serializing)]
        additional: Map<String, Value>,
    },
    #[serde(rename = "build-finished")]
    BuildFinished {
        success: bool,
        #[serde(flatten, skip_serializing)]
        additional: Map<String, Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CargoParseDiagnostic {
    pub line: usize,
    #[serde(skip_serializing)]
    pub detail: String,
    pub reason: CargoDiagnosticReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoDiagnosticReason {
    RecordTooLarge,
    ParseFailed,
    DuplicateArtifactIdentity,
}

pub fn parse_cargo_message(bytes: &[u8]) -> Result<CargoMessage, CargoParseDiagnostic> {
    if bytes.len() > MAX_LINE_BYTES {
        return Err(CargoParseDiagnostic {
            line: 0,
            detail: "Cargo JSON record exceeds capture bound".into(),
            reason: CargoDiagnosticReason::RecordTooLarge,
        });
    }
    serde_json::from_slice(bytes).map_err(|error| CargoParseDiagnostic {
        line: 0,
        detail: error.to_string(),
        reason: CargoDiagnosticReason::ParseFailed,
    })
}
