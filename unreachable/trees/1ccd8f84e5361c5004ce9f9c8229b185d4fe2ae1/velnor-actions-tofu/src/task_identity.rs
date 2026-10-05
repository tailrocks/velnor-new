//! Typed tofu task-identity extension (cache §1).
//!
//! Unknown schemas disable reuse; the orchestrator wraps the extension in
//! the stack-neutral envelope before selection.

use serde::Serialize;
use velnor_actions_contract::ContractError;

use crate::kinds::TofuTaskKind;
use crate::propose::{TOFU_DRIVER, TOFU_RUNNER};

/// Provenance of one digest slot (the root lockfile).
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

/// Typed tofu task-identity extension (cache §1); unknown schemas disable reuse.
#[derive(Debug, Clone, Serialize)]
pub struct TofuTaskIdentityExtension {
    /// Tofu root key (`root` or the root directory).
    pub unit_id: String,
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Execution profile (configuration) name.
    pub profile: String,
    /// Unit evidence path (`.` or the root directory).
    pub manifest: String,
    /// Module-graph digest.
    pub graph_digest: String,
    /// Normalized configured root (`""` or the root directory).
    pub root: String,
    /// Tofu task kind spelling.
    pub kind: String,
    /// Typed task kind (serialization keeps the string slot).
    #[serde(skip_serializing)]
    pub task_kind: TofuTaskKind,
    /// Driver plus runner combined (`driver+runner`).
    pub driver: String,
    /// Tool-input config digest.
    pub config_digest: String,
    /// Lockfile digest, when the root lockfile is available.
    pub lock_digest: Option<String>,
    /// Lockfile slot state (serialized; keeps Unknown distinct).
    pub lock_state: SlotState,
    /// Typed lockfile slot (serialization keeps the digest slot).
    #[serde(skip_serializing)]
    pub lock_slot: DigestSlot,
    /// The task may read undeclared inputs; disables reuse and coverage.
    pub undeclared_reads: bool,
    /// Declared non-tofu task inputs, sorted.
    pub declared_inputs: Vec<String>,
}

/// Inputs for deriving one task-identity extension before selection.
#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    /// Tofu root key.
    pub unit_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Unit evidence path.
    pub manifest: &'a str,
    /// Module-graph digest.
    pub graph_digest: &'a str,
    /// Normalized configured root.
    pub root: &'a str,
    /// Tool-input config digest.
    pub config_digest: &'a str,
    /// Root-lockfile slot, resolved against the checkout.
    pub lock_digest: DigestSlot,
    /// Tofu task kind.
    pub kind: TofuTaskKind,
    /// The task may read undeclared inputs.
    pub undeclared_reads: bool,
    /// Declared non-tofu task inputs.
    pub declared_inputs: &'a [String],
}

impl TofuTaskIdentityExtension {
    /// Wrap the extension in the stack-neutral envelope.
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: velnor_actions_contract::cachekey::TOFU_EXTENSION_SCHEMA.to_owned(),
            data: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }

    /// Reject reuse when inputs are undeclared or the lockfile is unresolved.
    ///
    /// Tofu init/validate task-result reuse is OFF (T23): even fully
    /// resolved extensions refuse, so a provider-cache hit still runs
    /// validation (`executed`, never `reused`). Fmt keeps the shared
    /// qualification: it never reads the lockfile or providers, and
    /// the T23 row scopes OFF to init/validate only.
    /// # Errors
    pub fn reuse_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs",
            ));
        }
        if self.lock_slot.is_unknown() {
            return Err(ContractError::identity(
                "stack_extension",
                "unresolved_input:lockfile",
            ));
        }
        if matches!(
            self.task_kind,
            TofuTaskKind::InitForValidate | TofuTaskKind::Validate
        ) {
            return Err(ContractError::identity(
                "stack_extension",
                "tofu_reuse_disabled",
            ));
        }
        Ok(())
    }

    /// Reject baseline coverage when inputs are undeclared or unresolved.
    ///
    /// Mirrors [`reuse_eligible`](Self::reuse_eligible): an unknown
    /// lockfile slot blocks coverage exactly like it blocks reuse.
    /// # Errors
    pub fn coverage_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs_no_coverage",
            ));
        }
        if self.lock_slot.is_unknown() {
            return Err(ContractError::identity(
                "stack_extension",
                "unresolved_input_no_coverage:lockfile",
            ));
        }
        Ok(())
    }

    /// Whether the root MUST execute conservatively (no reuse, no cover).
    #[must_use]
    pub fn conservative_execution_required(&self) -> bool {
        self.undeclared_reads || self.lock_slot.is_unknown()
    }

    /// Derive the extension for one task before selection and reuse.
    #[must_use]
    pub fn for_task(inputs: &ExtensionInputs<'_>) -> Self {
        Self {
            unit_id: inputs.unit_id.to_owned(),
            workspace_id: inputs.workspace_id.to_owned(),
            profile: inputs.profile.to_owned(),
            manifest: inputs.manifest.to_owned(),
            graph_digest: inputs.graph_digest.to_owned(),
            root: inputs.root.to_owned(),
            kind: inputs.kind.as_str().to_owned(),
            task_kind: inputs.kind,
            driver: format!("{TOFU_DRIVER}+{TOFU_RUNNER}"),
            config_digest: inputs.config_digest.to_owned(),
            lock_digest: inputs.lock_digest.as_known().map(str::to_owned),
            lock_state: inputs.lock_digest.state(),
            lock_slot: inputs.lock_digest.clone(),
            undeclared_reads: inputs.undeclared_reads,
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

/// Provider/plugin digest inputs for the tofu toolchain identity.
///
/// Sorted toolchain `components` entries binding the task's provider
/// surface declaration: which root's providers, which kind's lock
/// requirement, and which exact tofu pin. Init and validate share
/// the lock-reading slot; fmt binds the excluded marker (mirroring
/// [`lock_slot_for_kind`](crate::lockfile::lock_slot_for_kind)).
/// Provider CONTENT binds through the extension lock slot; this
/// declares the surface the toolchain must serve.
///
/// # Errors
///
/// Returns [`ContractError`] for kind spellings outside the known
/// tokens.
pub fn provider_toolchain_entries(
    unit_key: &str,
    kind_spelling: &str,
    tofu_spec: &str,
) -> Result<Vec<String>, ContractError> {
    use velnor_actions_contract::canonical::digest_b3_typed;
    use velnor_actions_contract::canonical_json_bytes;
    let slot = match TofuTaskKind::parse(kind_spelling)? {
        TofuTaskKind::Fmt => "excluded:kind_does_not_read_lockfile",
        TofuTaskKind::InitForValidate | TofuTaskKind::Validate => "lockfile",
    };
    let record = serde_json::json!({
        "schema": "tofu-provider-inputs-v1",
        "unit": unit_key,
        "kind_slot": slot,
        "tofu": tofu_spec,
    });
    let digest = digest_b3_typed(&canonical_json_bytes(&record)?);
    Ok(vec![format!("tofu-provider-inputs:{}", digest.as_str())])
}

/// Toolchain inputs for one tofu task: exact pin plus provider surface.
///
/// The orchestrator's tofu arm delegates here so the adapter owns
/// its toolchain shape; catalog specs sort inside, and unknown
/// kinds fail closed.
///
/// # Errors
///
/// Returns [`ContractError`] for kind spellings outside the known
/// tokens.
pub fn toolchain_inputs_for_task(
    task: &velnor_actions_contract::ProposedTask,
    specs: Vec<String>,
) -> Result<velnor_actions_contract::cachekey::ToolchainInputs, ContractError> {
    use velnor_actions_contract::cachekey::ToolchainInputs;
    let mut specs = specs;
    specs.sort();
    let spec = specs.first().map_or("", String::as_str);
    let components = provider_toolchain_entries(&task.identity.unit_key, &task.task_kind, spec)?;
    Ok(ToolchainInputs {
        tools: specs,
        components,
        compile_driver: task.identity.compile_driver.clone(),
        test_runner: task.identity.test_runner.clone(),
    })
}
