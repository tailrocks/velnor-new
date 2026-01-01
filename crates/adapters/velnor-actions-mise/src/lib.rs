//! Mise tool selection and pinned command construction.
//!
//! Facade over the mise family: execution substrate lives in
//! `velnor-actions-mise-core`, discovery in `-checks`, install inputs in
//! `-catalog`, task cache in `-cache`, Nextest plans in `-nextest`.
//!
//! Owns the subprocess/environment wrapper, pinned tool requests, the Git
//! discovery allowlist, and read-only task-cache helpers. Must not own Cargo
//! metadata parsing, Rust graph rules, `rust-toolchain.toml`, or GitHub YAML.

pub mod git;
pub use velnor_actions_mise_cache::{
    cache, cache_sources, cache_transport, cache_trust, gate6, restore, restore_evidence, reuse,
};
pub use velnor_actions_mise_catalog::{
    catalog, discovery, gh, preflight, requests, steps, toolfiles, wrappers,
};
pub use velnor_actions_mise_core::{
    check_deadline, checks, command, custom_run, error, runtime_paths, template,
};
pub use velnor_actions_mise_nextest::{
    build, nextest, nextest_config, nextest_plan, nextest_shapes, verify,
};
pub use velnor_actions_mise_probes::check_tool_probes;

pub use velnor_actions_mise_nextest::build::{
    CANDIDATE_BUILD_BIN, CANDIDATE_BUILD_PACKAGE, CandidateBuild,
};

pub use velnor_actions_mise_cache::cache::{
    CACHE_DIR_ENV, CachedTaskDescriptor, TASK_ARTIFACTS_DIR_NAME, TASK_ARTIFACTS_VERSION,
    TASK_CACHE_DIR_ENV, TASK_CACHE_MODE_ENV, TaskCacheMode, artifact_path, cache_mode_from_env,
    read_artifact_bytes, resolve_task_artifact_dir, task_artifact_dir_from_env,
    verify_artifact_digest,
};

pub use velnor_actions_mise_catalog::catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION,
    OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION, PinnedTool, RUST_TARGET_TRIPLE, RUST_VERSION,
    SHELLCHECK_VERSION, ToolCatalog, ZIZMOR_VERSION, check_freshness_requirements,
    validate_exact_version,
};

pub use velnor_actions_mise_core::check_deadline::CheckDeadline;

pub use velnor_actions_mise_catalog::discovery::{discover_checks, discover_checks_until};
pub use velnor_actions_mise_core::checks::{DiscoveredCheck, QualifiedCheck};

pub use velnor_actions_mise_core::command::{
    ALLOWED_MISE_SUBCOMMANDS, CREDENTIAL_ENV_KEYS, ENDPOINT_ENV_KEYS, ISOLATION_ENV,
    IsolatedCommand, MISE_CARGO_HOME_ENV, MISE_GLOBAL_FLAGS, MISE_RUSTUP_HOME_ENV,
    NO_AUTO_INSTALL_ENV, PROXY_ENV_KEYS, ProcessOutput, RUSTUP_TOOLCHAIN_ENV,
    TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
    TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, TOOL_COMMAND_SEPARATOR,
    is_allowed_mise_subcommand, toolchain_env,
};

pub use velnor_actions_mise_core::error::MiseError;

pub use velnor_actions_mise_cache::gate6::{
    Gate6Fixture, qualified_task_run_argv, render_gated_task_toml,
};

pub use velnor_actions_mise_catalog::gh::BaselineLookup;

pub use git::{ALLOWED_GIT_VERBS, GitRequest, is_allowed_git_verb};

pub use velnor_actions_mise_nextest::nextest::{
    ARCHIVE_FILE, NEXTEST_EXTRACT_BASE, NextestDriver, NextestPartition,
};

pub use velnor_actions_mise_nextest::nextest_config::{
    CI_PROFILE_NAME, DEFAULT_PROFILE_NAME, NEXTEST_CONFIG_REL, NextestConfig, NextestDiagnostic,
    parse_nextest_config,
};

pub use velnor_actions_mise_nextest::nextest_plan::{
    ArchiveIdentityInputs, ArchivePlan, SortedInventory, archive_identity, archive_write_required,
    count_inventory_tests, requires_archive_transfer,
};

pub use velnor_actions_mise_nextest::nextest_shapes::{NextestArchive, NextestList, NextestRun};

pub use velnor_actions_mise_catalog::preflight::{RouteDriver, RouteSelection, select_route};

pub use velnor_actions_mise_catalog::requests::{
    MetadataDiscovery, MetadataQualification, MiseInstall, PinnedToolExec,
};

pub use velnor_actions_mise_cache::restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, SaveInputs, ToolAvailability,
    fallback_for_error, save_decision, save_useful, verify_restored_task_result, writers_overlap,
};

pub use velnor_actions_mise_cache::restore_evidence::{
    RestoreObservation, classify_restore, output_bytes_complete,
};

pub use velnor_actions_mise_cache::reuse::{
    ReuseGrant, ReusePlan, ReuseQualification, ReuseSignal, TaskArtifactTransport, TaskCacheKey,
    TaskReuseRequest, plan_reuse,
};

pub use velnor_actions_mise_catalog::steps::{
    PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP, PreparePinnedTools,
    PrepareRustComponents, ToolHomes, VERIFY_PREPARED_INPUTS_STEP, VerifyPreparedInputs,
};

pub use velnor_actions_mise_catalog::steps_rust_target::{
    PREPARE_RUST_TARGET_STEP, PrepareRustTarget,
};

pub use velnor_actions_mise_core::template::TaskTemplate;

pub use velnor_actions_mise_catalog::toolfiles::{
    DOT_MISE_TOML_FILE, FOREIGN_TOOL_FILES, MISE_ENV_PREFIX, MISE_LOCK_FILE, MISE_TOML_FILE,
    MISSING_RECOMMENDED_INPUT, MiseInspection, MiseSpec, OWNED_SYMBOLS, TOOLING_INPUT_INVALID,
    ToolFile, ToolFinding, ToolInspectError, inspect_mise_file, is_mise_env_symbol,
    is_owned_mise_file, lock_tool_versions, stack_for_symbol,
};

pub use velnor_actions_mise_nextest::verify::{
    TestRunner, VERIFY_TOOLCHAIN_STEP, VerifySpec, VerifyToolchain,
};

pub use velnor_actions_mise_catalog::wrappers::{
    CargoWrapper, MBX_COMMAND, MBX_SHIM_ENV, WrapperDiagnostic, is_mbx_command, parse_cargo_wrapper,
};

/// Stable identifier for the Mise tool wrapper.
pub const TOOL_ID: &str = "mise";
