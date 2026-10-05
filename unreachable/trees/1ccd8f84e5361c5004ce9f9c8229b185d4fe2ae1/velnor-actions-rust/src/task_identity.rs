//! Typed Rust task-identity extension (cache §1).
//!
//! Unknown schemas disable reuse; the orchestrator wraps the extension in
//! the stack-neutral envelope before selection.

use serde::Serialize;
use velnor_actions_contract::ContractError;

use crate::profile::{CompileDriver, TestRunner};
use crate::tasks::TaskKind;

/// Provenance of one digest slot (lockfile, Nextest config).
///
/// Only [`DigestSlot::Known`] binds content; [`DigestSlot::AbsentProven`]
/// binds proven absence, and [`DigestSlot::Unknown`] blocks reuse and
/// coverage until the orchestrator resolves the slot. Absence and
/// ignorance never collapse: an unobserved lockfile is unknown, never
/// proven absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestSlot {
    /// Content digest bound.
    Known(String),
    /// Proven absent, with evidence.
    AbsentProven(String),
    /// Unresolved, with reason; blocks reuse and coverage.
    Unknown(String),
}

impl DigestSlot {
    /// The bound digest, when this slot binds content.
    #[must_use]
    pub fn as_known(&self) -> Option<&str> {
        match self {
            Self::Known(digest) => Some(digest),
            Self::AbsentProven(_) | Self::Unknown(_) => None,
        }
    }

    /// Whether this slot blocks reuse and coverage (unknown only).
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    /// Serializable discriminant of this slot.
    #[must_use]
    pub fn state(&self) -> SlotState {
        match self {
            Self::Known(_) => SlotState::Known,
            Self::AbsentProven(_) => SlotState::AbsentProven,
            Self::Unknown(_) => SlotState::Unknown,
        }
    }
}

/// Serializable discriminant of one [`DigestSlot`].
///
/// Evidence and reasons stay diagnostic-only (never serialized into
/// the identity preimage); the state preserves the Unknown-vs-absence
/// distinction that `Option<digest>` erases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    /// Content digest bound.
    Known,
    /// Proven absent, with evidence.
    AbsentProven,
    /// Unresolved; blocks reuse and coverage.
    Unknown,
}

/// Typed Rust task-identity extension (cache §1); unknown schemas disable reuse.
#[derive(Debug, Clone, Serialize)]
pub struct RustTaskIdentityExtension {
    /// Cargo package ID.
    pub package_id: String,
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Execution profile (configuration) name.
    pub profile: String,
    /// Normalized manifest path.
    pub manifest: String,
    /// Workspace/local-package graph digest.
    pub graph_digest: String,
    /// Target kinds and names, sorted.
    pub targets: Vec<String>,
    /// Enabled features, sorted.
    pub features: Vec<String>,
    /// Rust target and profile.
    pub target: String,
    /// Compile driver plus test runner (`driver+runner`).
    pub driver: String,
    /// Typed compile driver (serialization keeps the composite slot).
    #[serde(skip_serializing)]
    pub compile_driver: CompileDriver,
    /// Typed test runner (serialization keeps the composite slot).
    #[serde(skip_serializing)]
    pub test_runner: TestRunner,
    /// Cargo config and build-script input digests.
    pub config_digest: String,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<String>,
    /// Nextest-config slot state (serialized; keeps Unknown distinct).
    pub nextest_state: SlotState,
    /// Typed Nextest-config slot (serialization keeps the digest slot).
    #[serde(skip_serializing)]
    pub nextest_slot: DigestSlot,
    /// Rust task kind plus test/archive identity.
    pub kind: String,
    /// Typed task kind (serialization keeps the string slot).
    #[serde(skip_serializing)]
    pub task_kind: TaskKind,
    /// Build script reads undeclared inputs; disables reuse and coverage.
    pub undeclared_reads: bool,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<String>,
    /// Lockfile slot state (serialized; keeps Unknown distinct).
    pub lock_state: SlotState,
    /// Typed lockfile slot (serialization keeps the digest slot).
    #[serde(skip_serializing)]
    pub lock_slot: DigestSlot,
    /// Archive identity (producing build task id) for Nextest archives only.
    pub archive: Option<String>,
    /// Declared `rerun-if-changed` build inputs, sorted.
    pub rerun_inputs: Vec<String>,
    /// Declared non-Rust task inputs, sorted.
    pub declared_inputs: Vec<String>,
}

/// Inputs for deriving one task-identity extension before selection.
#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    /// Cargo package ID.
    pub package_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Normalized manifest path.
    pub manifest: &'a str,
    /// Workspace/local-package graph digest.
    pub graph_digest: &'a str,
    /// Target kinds and names.
    pub targets: &'a [String],
    /// Enabled features.
    pub features: &'a [String],
    /// Rust target and profile.
    pub target: &'a str,
    /// Compile driver.
    pub driver: CompileDriver,
    /// Test runner.
    pub runner: TestRunner,
    /// Cargo config and build-script input digests.
    pub config_digest: &'a str,
    /// `Cargo.lock` slot, resolved against the checkout.
    pub lock_digest: DigestSlot,
    /// `.config/nextest.toml` slot for Nextest profiles.
    pub nextest_digest: DigestSlot,
    /// Rust task kind.
    pub kind: TaskKind,
    /// Build task id producing the archive (Nextest `Build` only).
    pub archive_source: Option<&'a str>,
    /// Declared `rerun-if-changed` inputs (`None` means unknown).
    pub rerun_inputs: Option<&'a [String]>,
    /// Whether the package carries a build script.
    pub has_build_script: bool,
    /// Declared non-Rust task inputs.
    pub declared_inputs: &'a [String],
}

impl RustTaskIdentityExtension {
    /// Wrap the extension in the stack-neutral envelope.
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: "rust-task-identity-v1".to_owned(),
            data: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }

    /// Reject reuse when build inputs are undeclared or unresolved.
    ///
    /// Unresolved slots fail closed through the same inventory
    /// [`unresolved_inputs`](crate::identity::unresolved_inputs) reports,
    /// so the gate and the inventory cannot drift apart.
    /// # Errors
    pub fn reuse_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs",
            ));
        }
        if let Some(input) = self.first_blocking_input() {
            return Err(ContractError::identity(
                "stack_extension",
                format!("unresolved_input:{}", input.as_str()),
            ));
        }
        Ok(())
    }

    /// Derive the extension for one task group before selection and reuse.
    ///
    /// Archives attach only to Nextest `Build` groups (cargo-test never
    /// archives; the archive carries its producing build task id so trust
    /// and retention follow the source). A build script with unknown
    /// `rerun-if-changed` inputs conservatively disables reuse.
    #[must_use]
    pub fn for_task(inputs: &ExtensionInputs<'_>) -> Self {
        let nextest = inputs.runner == TestRunner::CargoNextest;
        let archive = if inputs.kind == TaskKind::Build && nextest {
            inputs.archive_source.map(str::to_owned)
        } else {
            None
        };
        Self {
            package_id: inputs.package_id.to_owned(),
            workspace_id: inputs.workspace_id.to_owned(),
            profile: inputs.profile.to_owned(),
            manifest: inputs.manifest.to_owned(),
            graph_digest: inputs.graph_digest.to_owned(),
            targets: sorted_unique(inputs.targets),
            features: sorted_unique(inputs.features),
            target: inputs.target.to_owned(),
            driver: format!("{}+{}", inputs.driver.as_str(), inputs.runner.as_str()),
            compile_driver: inputs.driver,
            test_runner: inputs.runner,
            config_digest: inputs.config_digest.to_owned(),
            nextest_digest: inputs.nextest_digest.as_known().map(str::to_owned),
            nextest_state: inputs.nextest_digest.state(),
            nextest_slot: inputs.nextest_digest.clone(),
            kind: inputs.kind.as_str().to_owned(),
            task_kind: inputs.kind,
            undeclared_reads: inputs.has_build_script && inputs.rerun_inputs.is_none(),
            lock_digest: inputs.lock_digest.as_known().map(str::to_owned),
            lock_state: inputs.lock_digest.state(),
            lock_slot: inputs.lock_digest.clone(),
            archive,
            rerun_inputs: inputs.rerun_inputs.map_or_else(Vec::new, sorted_unique),
            declared_inputs: sorted_unique(inputs.declared_inputs),
        }
    }
}

/// Sorted deduped copy for identity stability.
fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out.dedup();
    out
}
