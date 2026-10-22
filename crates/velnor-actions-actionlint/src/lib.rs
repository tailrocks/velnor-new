//! Workflow-lint adapter: pinned jactionlint config, capabilities and
//! argument vectors, plus the pinned action refs and their schemas.
//!
//! Owns generated `.github/jactionlint.yaml` bytes, the pinned jactionlint
//! capability flags and lint argv, pinned action refs with override/input
//! validation, and lint-tool pins. Pure
//! data plus validation: never spawns processes, never reads the
//! filesystem; Mise executes tools while the orchestrator invokes this
//! crate for config and validation. The crate keeps its historical
//! `actionlint` name; jactionlint is the only workflow linter.

pub mod actions;
pub mod capabilities;
pub mod config;
pub mod error;
pub mod metadata;
pub mod overrides;
pub mod tools;

pub use actions::{
    ALINT_ACTION, ALINT_ACTION_SHA, ALINT_ACTION_VERSION, ALLOWED_ACTIONS, ATTEST_ACTION,
    ATTEST_ACTION_SHA, ATTEST_ACTION_VERSION, ATTEST_BUILD_PROVENANCE_ACTION,
    ATTEST_BUILD_PROVENANCE_ACTION_SHA, ATTEST_BUILD_PROVENANCE_ACTION_VERSION,
    ATTEST_BUILD_PROVENANCE_USES, ATTEST_USES, AWS_CREDENTIALS_ACTION, AWS_CREDENTIALS_ACTION_SHA,
    AWS_CREDENTIALS_ACTION_VERSION, CHECKOUT_ACTION, CHECKOUT_ACTION_USES,
    CREATE_GITHUB_APP_TOKEN_ACTION, CREATE_GITHUB_APP_TOKEN_ACTION_SHA,
    CREATE_GITHUB_APP_TOKEN_ACTION_VERSION, CREATE_GITHUB_APP_TOKEN_USES,
    DOWNLOAD_ARTIFACT_ACTION_SHA, DOWNLOAD_ARTIFACT_ACTION_USES, DOWNLOAD_ARTIFACT_ACTION_VERSION,
    GITHUB_SCRIPT_ACTION, GITHUB_SCRIPT_ACTION_SHA, GITHUB_SCRIPT_ACTION_USES,
    GITHUB_SCRIPT_ACTION_VERSION, MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION,
    PinnedActionRef, UPLOAD_ARTIFACT_ACTION_SHA, UPLOAD_ARTIFACT_ACTION_USES,
    UPLOAD_ARTIFACT_ACTION_VERSION,
};
pub use capabilities::{
    JACTIONLINT_VERSION, JactionlintCapabilities, StepSyntax, is_known_hosted_label,
};
pub use config::{JactionlintConfigInput, render_jactionlint_yaml};
pub use error::ActionlintError;
pub use metadata::{
    FOREIGN_TOOL_FILES, JACTIONLINT_CONFIG_FILE, OWNED_SYMBOLS, is_owned_jactionlint_file,
    stack_for_symbol,
};
pub use overrides::{
    ActionInputSchema, ActionPinOverride, ApprovedPin, ApprovedPinCatalog, checkout_inputs_schema,
    validate_action_inputs,
};
pub use tools::{
    JACTIONLINT_CONFIG_PATH, JactionlintToolchain, LINT_FILE_GLOBS, LINT_FLAGS, SHELLCHECK_VERSION,
    ShellcheckToolchain, WorkflowLintTools, workflow_lint_script,
};

/// Stable identifier for the workflow-lint tool metadata.
pub const TOOL_ID: &str = "jactionlint";
