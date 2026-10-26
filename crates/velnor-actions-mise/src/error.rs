// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Result as FmtResult};
use velnor_actions_contract::ContractError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MiseError {
    EmptyCommand {
        program: String,
    },
    EmptyToolchain,
    ForbiddenPayload {
        program: String,
        reason: String,
    },
    InvalidNextestInput {
        field: String,
        value: String,
    },
    UnknownTool {
        tool: String,
    },
    InvalidToolVersion {
        tool: String,
        version: String,
    },
    GitVerbRejected {
        verb: String,
    },
    InvalidManifestPath {
        path: String,
    },
    SpawnFailed {
        program: String,
        message: String,
    },
    NonZeroExit {
        program: String,
        code: Option<i32>,
        stderr: String,
    },
    InvalidUtf8 {
        program: String,
        stream: String,
    },
    UnknownCacheMode {
        mode: String,
    },
    CacheNotEligible {
        task: String,
        reason: String,
    },
    ArtifactEscapesRoot {
        path: String,
    },
    ArtifactNotFound {
        path: String,
    },
    ArtifactUnreadable {
        path: String,
        message: String,
    },
    InvalidDigest {
        value: String,
        problem: String,
    },
    DigestMismatch {
        expected: String,
        actual: String,
    },
    Contract {
        problem: String,
    },
    InvalidStepInput {
        field: String,
        value: String,
    },
    MissingToolPin {
        spec: String,
        installed: Vec<String>,
    },
}

impl Display for MiseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::EmptyCommand { program } => write!(f, "empty_command: {program}"),
            Self::EmptyToolchain => write!(f, "empty_toolchain"),
            Self::ForbiddenPayload { program, reason } => {
                write!(f, "forbidden_payload: {program}: {reason}")
            }
            Self::InvalidNextestInput { field, value } => {
                write!(f, "invalid_nextest_input: {field}: {value}")
            }
            Self::UnknownTool { tool } => write!(f, "unknown_tool: {tool}"),
            Self::InvalidToolVersion { tool, version } => {
                write!(f, "invalid_tool_version: {tool}: {version}")
            }
            Self::GitVerbRejected { verb } => write!(f, "git_verb_rejected: {verb}"),
            Self::InvalidManifestPath { path } => write!(f, "invalid_manifest_path: {path}"),
            Self::SpawnFailed { program, message } => {
                write!(f, "spawn_failed: {program}: {message}")
            }
            Self::NonZeroExit {
                program,
                code,
                stderr,
            } => write!(f, "nonzero_exit: {program}: {code:?}: {stderr}"),
            Self::InvalidUtf8 { program, stream } => {
                write!(f, "invalid_utf8: {program}: {stream}")
            }
            Self::UnknownCacheMode { mode } => write!(f, "unknown_cache_mode: {mode}"),
            Self::CacheNotEligible { task, reason } => {
                write!(f, "cache_not_eligible: {task}: {reason}")
            }
            Self::ArtifactEscapesRoot { path } => write!(f, "artifact_escapes_root: {path}"),
            Self::ArtifactNotFound { path } => write!(f, "artifact_not_found: {path}"),
            Self::ArtifactUnreadable { path, message } => {
                write!(f, "artifact_unreadable: {path}: {message}")
            }
            Self::InvalidDigest { value, problem } => {
                write!(f, "invalid_digest: {value}: {problem}")
            }
            Self::DigestMismatch { expected, actual } => {
                write!(f, "digest_mismatch: expected {expected}, got {actual}")
            }
            Self::Contract { problem } => write!(f, "contract: {problem}"),
            Self::InvalidStepInput { field, value } => {
                write!(f, "invalid_step_input: {field}: {value}")
            }
            Self::MissingToolPin { spec, installed } => {
                if installed.is_empty() {
                    write!(
                        f,
                        "missing_tool_pin: {spec} not installed (installed: none)"
                    )
                } else {
                    write!(
                        f,
                        "missing_tool_pin: {spec} not installed (installed: {})",
                        installed.join(", ")
                    )
                }
            }
        }
    }
}

impl std::error::Error for MiseError {}

impl From<MiseError> for ContractError {
    fn from(error: MiseError) -> Self {
        match &error {
            MiseError::UnknownTool { .. } | MiseError::InvalidToolVersion { .. } => {
                Self::identity("mise_tool", error.to_string())
            }
            MiseError::UnknownCacheMode { .. }
            | MiseError::CacheNotEligible { .. }
            | MiseError::InvalidDigest { .. }
            | MiseError::DigestMismatch { .. } => Self::identity("task_cache", error.to_string()),
            MiseError::InvalidManifestPath { .. }
            | MiseError::ArtifactEscapesRoot { .. }
            | MiseError::ArtifactNotFound { .. }
            | MiseError::ArtifactUnreadable { .. } => {
                Self::identity("mise_path", error.to_string())
            }
            _ => Self::identity("mise_command", error.to_string()),
        }
    }
}
