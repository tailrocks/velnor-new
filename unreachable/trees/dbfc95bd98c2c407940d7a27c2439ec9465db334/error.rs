//! Typed errors for the Mise subprocess wrapper and tool requests.

use std::fmt::{Display, Formatter, Result as FmtResult};
use velnor_actions_contract::ContractError;

/// Errors for isolated command construction, pinned tool requests, the Git
/// verb allowlist, and read-only task-cache helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MiseError {
    /// A command was built with no payload after the program or separator.
    EmptyCommand {
        /// Program that received the empty payload.
        program: String,
    },
    /// A pinned tool request named zero tools; Velnor always pins at least one.
    EmptyToolchain,
    /// A payload names a forbidden program (`rustup`) or installer action.
    ForbiddenPayload {
        /// Rejected program as supplied.
        program: String,
        /// Machine-readable reason (`rustup_forbidden`, `cargo_install_forbidden`).
        reason: String,
    },
    /// A Nextest shape input (package, partition, key) is malformed.
    InvalidNextestInput {
        /// Rejected field name.
        field: String,
        /// Rejected value as supplied.
        value: String,
    },
    /// A tool name is not in the Velnor pinned catalog.
    UnknownTool {
        /// Rejected tool name.
        tool: String,
    },
    /// A tool version is not an exact `major.minor.patch` pin.
    InvalidToolVersion {
        /// Tool the version was supplied for.
        tool: String,
        /// Rejected version string.
        version: String,
    },
    /// A Git verb is outside the `rev-parse`/`ls-files`/`diff`/`show` allowlist.
    GitVerbRejected {
        /// Rejected verb.
        verb: String,
    },
    /// A Cargo manifest path is empty.
    InvalidManifestPath {
        /// Rejected path as supplied.
        path: String,
    },
    /// The process could not be spawned or reaped.
    SpawnFailed {
        /// Program that failed to launch.
        program: String,
        /// Operating-system error detail.
        message: String,
    },
    /// The process exited with a nonzero status.
    NonZeroExit {
        /// Program that failed.
        program: String,
        /// Exit code when the platform reports one.
        code: Option<i32>,
        /// Captured standard error.
        stderr: String,
    },
    /// A captured stream is not valid UTF-8 where text was required.
    InvalidUtf8 {
        /// Program that produced the stream.
        program: String,
        /// Stream name (`stdout` or `stderr`).
        stream: String,
    },
    /// A task-cache mode string is not one of the five qualified modes.
    UnknownCacheMode {
        /// Rejected mode string.
        mode: String,
    },
    /// A task declares no cacheable inputs, so artifact caching is ineligible.
    CacheNotEligible {
        /// Task that cannot be cached.
        task: String,
        /// Machine-readable reason.
        reason: String,
    },
    /// A cache artifact path escapes its artifact root.
    ArtifactEscapesRoot {
        /// Rejected path as supplied.
        path: String,
    },
    /// A cache artifact file does not exist.
    ArtifactNotFound {
        /// Missing path as supplied.
        path: String,
    },
    /// A cache artifact file exists but cannot be read.
    ArtifactUnreadable {
        /// Unreadable path as supplied.
        path: String,
        /// Operating-system error detail.
        message: String,
    },
    /// A digest string is not a well-formed `b3-` digest.
    InvalidDigest {
        /// Rejected digest string.
        value: String,
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Artifact bytes do not match the expected digest.
    DigestMismatch {
        /// Expected digest.
        expected: String,
        /// Digest of the observed bytes.
        actual: String,
    },
    /// A contract helper (`canonical_json`, digest) rejected an input.
    Contract {
        /// Contract error detail.
        problem: String,
    },
    /// A typed-step input (tool home, target, platform) is blank or malformed.
    InvalidStepInput {
        /// Rejected field name.
        field: String,
        /// Rejected value as supplied.
        value: String,
    },
    /// A baseline-lookup input (base SHA, workflow, branch, artifact) is malformed.
    InvalidBaselineInput {
        /// Rejected field name.
        field: String,
        /// Rejected value as supplied.
        value: String,
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
            Self::InvalidBaselineInput { field, value } => {
                write!(f, "invalid_baseline_input: {field}: {value}")
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
