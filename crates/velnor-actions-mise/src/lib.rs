//! Mise tool selection and pinned command construction.
//!
//! Owns the subprocess/environment wrapper, pinned tool requests, the Git
//! discovery allowlist, and read-only task-cache helpers. Must not own Cargo
//! metadata parsing, Rust graph rules, `rust-toolchain.toml`, or GitHub YAML.

pub mod build;
pub mod cache;
pub mod cache_sources;
pub mod cache_transport;
pub mod cache_trust;
pub mod catalog;
pub mod check_deadline;
pub mod checks;
pub mod command;
pub mod custom_run;
pub mod error;
pub mod gate6;
pub mod gh;
pub mod git;
pub mod nextest;
pub mod nextest_config;
pub mod nextest_plan;
pub mod nextest_shapes;
pub mod preflight;
pub mod requests;
pub mod restore;
pub mod restore_evidence;
pub mod reuse;
pub mod runtime_paths;
pub mod steps;
pub mod steps_rust_target;
pub mod template;
mod toml_parser;
mod toml_scan;
mod toml_strings;
pub mod toolfiles;
pub mod verify;
pub mod wrappers;

pub use build::{CANDIDATE_BUILD_BIN, CANDIDATE_BUILD_PACKAGE, CandidateBuild};
pub use cache::{
    CACHE_DIR_ENV, CachedTaskDescriptor, TASK_ARTIFACTS_DIR_NAME, TASK_ARTIFACTS_VERSION,
    TASK_CACHE_DIR_ENV, TASK_CACHE_MODE_ENV, TaskCacheMode, artifact_path, cache_mode_from_env,
    read_artifact_bytes, resolve_task_artifact_dir, task_artifact_dir_from_env,
    verify_artifact_digest,
};
pub use catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION,
    OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION, PinnedTool, RUST_TARGET_TRIPLE, RUST_VERSION,
    SHELLCHECK_VERSION, ToolCatalog, ZIZMOR_VERSION, check_freshness_requirements,
    validate_exact_version,
};
pub use check_deadline::CheckDeadline;
pub use checks::{DiscoveredCheck, QualifiedCheck, discover_checks, discover_checks_until};
pub use command::{
    ALLOWED_MISE_SUBCOMMANDS, CREDENTIAL_ENV_KEYS, ENDPOINT_ENV_KEYS, ISOLATION_ENV,
    IsolatedCommand, MISE_CARGO_HOME_ENV, MISE_GLOBAL_FLAGS, MISE_RUSTUP_HOME_ENV,
    NO_AUTO_INSTALL_ENV, PROXY_ENV_KEYS, ProcessOutput, RUSTUP_TOOLCHAIN_ENV,
    TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
    TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, TOOL_COMMAND_SEPARATOR,
    is_allowed_mise_subcommand, toolchain_env,
};
pub use error::MiseError;
pub use gate6::{Gate6Fixture, qualified_task_run_argv, render_gated_task_toml};
pub use gh::BaselineLookup;
pub use git::{ALLOWED_GIT_VERBS, GitRequest, is_allowed_git_verb};
pub use nextest::{ARCHIVE_FILE, NEXTEST_EXTRACT_BASE, NextestDriver, NextestPartition};
pub use nextest_config::{
    CI_PROFILE_NAME, DEFAULT_PROFILE_NAME, NEXTEST_CONFIG_REL, NextestConfig, NextestDiagnostic,
    parse_nextest_config,
};
pub use nextest_plan::{
    ArchiveIdentityInputs, ArchivePlan, SortedInventory, archive_identity, archive_write_required,
    count_inventory_tests, requires_archive_transfer,
};
pub use nextest_shapes::{NextestArchive, NextestList, NextestRun};
pub use preflight::{RouteDriver, RouteSelection, select_route};
pub use requests::{MetadataDiscovery, MetadataQualification, MiseInstall, PinnedToolExec};
pub use restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, SaveInputs, ToolAvailability,
    fallback_for_error, save_decision, save_useful, verify_restored_task_result, writers_overlap,
};
pub use restore_evidence::{RestoreObservation, classify_restore, output_bytes_complete};
pub use reuse::{
    ReuseGrant, ReusePlan, ReuseQualification, ReuseSignal, TaskArtifactTransport, TaskCacheKey,
    TaskReuseRequest, plan_reuse,
};
pub use steps::{
    PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP, PreparePinnedTools,
    PrepareRustComponents, ToolHomes, VERIFY_PREPARED_INPUTS_STEP, VerifyPreparedInputs,
};
pub use steps_rust_target::{PREPARE_RUST_TARGET_STEP, PrepareRustTarget};
pub use template::TaskTemplate;
pub use toolfiles::{
    DOT_MISE_TOML_FILE, FOREIGN_TOOL_FILES, MISE_ENV_PREFIX, MISE_LOCK_FILE, MISE_TOML_FILE,
    MISSING_RECOMMENDED_INPUT, MiseInspection, MiseSpec, OWNED_SYMBOLS, TOOLING_INPUT_INVALID,
    ToolFile, ToolFinding, ToolInspectError, inspect_mise_file, is_mise_env_symbol,
    is_owned_mise_file, lock_tool_versions, stack_for_symbol,
};
pub use verify::{TestRunner, VERIFY_TOOLCHAIN_STEP, VerifySpec, VerifyToolchain};
pub use wrappers::{
    CargoWrapper, MBX_COMMAND, MBX_SHIM_ENV, WrapperDiagnostic, is_mbx_command, parse_cargo_wrapper,
};

/// Stable identifier for the Mise tool wrapper.
pub const TOOL_ID: &str = "mise";
pub mod check_tool_probes;
