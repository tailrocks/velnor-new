//! Mise tool selection and pinned command construction.
//!
//! Owns the subprocess/environment wrapper, pinned tool requests, the Git
//! discovery allowlist, and read-only task-cache helpers. Must not own Cargo
//! metadata parsing, Rust graph rules, `rust-toolchain.toml`, or GitHub YAML.

pub mod build;
pub mod cache;
pub mod catalog;
pub mod command;
pub mod error;
pub mod gate6;
pub mod gh;
pub mod git;
pub mod nextest;
pub mod nextest_plan;
pub mod preflight;
pub mod requests;
pub mod restore;
pub mod reuse;
pub mod steps;
pub mod template;
pub mod verify;

pub use build::{CANDIDATE_BUILD_BIN, CANDIDATE_BUILD_PACKAGE, CandidateBuild};
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
    ALLOWED_MISE_SUBCOMMANDS, ISOLATION_ENV, IsolatedCommand, MISE_CARGO_HOME_ENV,
    MISE_GLOBAL_FLAGS, MISE_RUSTUP_HOME_ENV, NO_AUTO_INSTALL_ENV, ProcessOutput,
    RUSTUP_TOOLCHAIN_ENV, TOOL_COMMAND_SEPARATOR, is_allowed_mise_subcommand, toolchain_env,
};
pub use error::MiseError;
pub use gate6::{Gate6Fixture, qualified_task_run_argv, render_gated_task_toml};
pub use gh::BaselineLookup;
pub use git::{ALLOWED_GIT_VERBS, GitRequest, is_allowed_git_verb};
pub use nextest::{
    ARCHIVE_FILE, NEXTEST_EXTRACT_BASE, NextestArchive, NextestDriver, NextestList,
    NextestPartition, NextestRun,
};
pub use nextest_plan::{ArchivePlan, SortedInventory};
pub use preflight::{RouteDriver, RouteProof, prove_route};
pub use requests::{MetadataDiscovery, MetadataQualification, MiseInstall, PinnedToolExec};
pub use restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, ReusePlan, ToolAvailability,
    fallback_for_error, plan_reuse, verify_restored_task_result,
};
pub use reuse::{
    ReuseGrant, ReuseQualification, ReuseSignal, TaskArtifactTransport, TaskCacheKey,
    TaskReuseRequest,
};
pub use steps::{
    PREPARE_PINNED_TOOLS_STEP, PreparePinnedTools, ToolHomes, VERIFY_PREPARED_INPUTS_STEP,
    VerifyPreparedInputs,
};
pub use template::TaskTemplate;
pub use verify::{TestRunner, VERIFY_TOOLCHAIN_STEP, VerifySpec, VerifyToolchain};

/// Stable identifier for the Mise tool wrapper.
pub const TOOL_ID: &str = "mise";
