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
    ALINT_ACTION, ALINT_ACTION_SHA, ALINT_ACTION_VERSION, ALLOWED_ACTIONS, AWS_CREDENTIALS_ACTION,
    AWS_CREDENTIALS_ACTION_SHA, AWS_CREDENTIALS_ACTION_VERSION, CHECKOUT_ACTION, PinnedActionRef,
};
pub use capabilities::{
    ACTIONLINT_VERSION, ActionlintCapabilities, NativeParallelismConcerns, StepSyntax,
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
    ZizmorToolchain,
};

/// Stable identifier for the actionlint tool metadata.
pub const TOOL_ID: &str = "actionlint";
