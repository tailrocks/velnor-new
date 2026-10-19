//! Pinned actionlint configuration, capability, and action-schema types.
//!
//! Owns generated `actionlint.yaml` bytes, the pinned actionlint
//! capability flags, pinned action refs with override/input validation,
//! and lint-tool pins. Pure data plus validation: never spawns
//! processes, never reads the filesystem; Mise executes tools while the
//! orchestrator invokes this crate for config and validation.

pub mod actions;
pub mod capabilities;
pub mod config;
pub mod error;
pub mod metadata;
pub mod overrides;
pub mod tools;
pub mod zizmor;

pub use actions::{
    ALINT_ACTION, ALINT_ACTION_SHA, ALINT_ACTION_VERSION, ALLOWED_ACTIONS,
    ATTEST_BUILD_PROVENANCE_ACTION, ATTEST_BUILD_PROVENANCE_ACTION_SHA,
    ATTEST_BUILD_PROVENANCE_ACTION_VERSION, ATTEST_BUILD_PROVENANCE_USES, AWS_CREDENTIALS_ACTION,
    AWS_CREDENTIALS_ACTION_SHA, AWS_CREDENTIALS_ACTION_VERSION, CHECKOUT_ACTION,
    CHECKOUT_ACTION_USES, CREATE_GITHUB_APP_TOKEN_ACTION, CREATE_GITHUB_APP_TOKEN_ACTION_SHA,
    CREATE_GITHUB_APP_TOKEN_ACTION_VERSION, CREATE_GITHUB_APP_TOKEN_USES,
    DOWNLOAD_ARTIFACT_ACTION_SHA, DOWNLOAD_ARTIFACT_ACTION_USES, DOWNLOAD_ARTIFACT_ACTION_VERSION,
    GITHUB_SCRIPT_ACTION, GITHUB_SCRIPT_ACTION_SHA, GITHUB_SCRIPT_ACTION_USES,
    GITHUB_SCRIPT_ACTION_VERSION, MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION,
    PinnedActionRef, UPLOAD_ARTIFACT_ACTION_SHA, UPLOAD_ARTIFACT_ACTION_USES,
    UPLOAD_ARTIFACT_ACTION_VERSION,
};
pub use capabilities::{
    ACTIONLINT_VERSION, ActionlintCapabilities, STEP_PARALLELISM_IGNORES, StepSyntax,
};
pub use config::{
    ActionlintConfigInput, ActionlintConfigOutput, IgnoreEntry, IgnorePolicy, RUNNER_LABEL_BRIDGE,
    render_actionlint_yaml,
};
pub use error::ActionlintError;
pub use metadata::{
    ACTIONLINT_CONFIG_FILE, FOREIGN_TOOL_FILES, OWNED_SYMBOLS, is_owned_actionlint_file,
    stack_for_symbol,
};
pub use overrides::{
    ActionInputSchema, ActionPinOverride, ApprovedPin, ApprovedPinCatalog, checkout_inputs_schema,
    validate_action_inputs,
};
pub use tools::{
    ActionlintToolchain, SHELLCHECK_VERSION, ShellcheckToolchain, WorkflowLintTools,
    ZizmorToolchain, step_parallelism_ignore_args, workflow_lint_args,
};

/// Stable identifier for the actionlint tool metadata.
pub const TOOL_ID: &str = "actionlint";
