//! Mise tool selection and pinned command construction.
//!
//! Owns the subprocess/environment wrapper, pinned tool requests, the Git
//! discovery allowlist, and read-only task-cache helpers. Must not own Cargo
//! metadata parsing, Rust graph rules, `rust-toolchain.toml`, or GitHub YAML.

pub mod cache;
pub mod catalog;
pub mod command;
pub mod error;
pub mod git;
pub mod requests;

pub use cache::{
    CACHE_DIR_ENV, CachedTaskDescriptor, TASK_ARTIFACTS_DIR_NAME, TASK_ARTIFACTS_VERSION,
    TASK_CACHE_DIR_ENV, TASK_CACHE_MODE_ENV, TaskCacheMode, artifact_path, cache_mode_from_env,
    read_artifact_bytes, resolve_task_artifact_dir, task_artifact_dir_from_env,
    verify_artifact_digest,
};
pub use catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION, PinnedTool, RUST_VERSION,
    SHELLCHECK_VERSION, ToolCatalog, ZIZMOR_VERSION, validate_exact_version,
};
pub use command::{
    ISOLATION_ENV, IsolatedCommand, MISE_GLOBAL_FLAGS, ProcessOutput, TOOL_COMMAND_SEPARATOR,
};
pub use error::MiseError;
pub use git::{ALLOWED_GIT_VERBS, GitRequest, is_allowed_git_verb};
pub use requests::{MetadataDiscovery, MetadataQualification, PinnedToolExec};

/// Stable identifier for the Mise tool wrapper.
pub const TOOL_ID: &str = "mise";
