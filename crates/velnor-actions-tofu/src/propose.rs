//! Tofu proposal contract: configured roots into neutral proposals.
//!
//! [`propose_task`] converts one derived [`TofuTaskGroup`] wholesale
//! into a [`ProposedTask`]: task IDs through the existing
//! [`task_id_for_stack`](velnor_actions_contract::task_id_for_stack)
//! grammar, `Validate depends_on same-root Init` edges through the
//! neutral `depends_on` vocabulary, and precomputed adapter facts
//! (payload, environment, component, project root), never recomputed
//! downstream. The kind/tool helpers below back the orchestrator's
//! closed per-stack dispatch: spellings stay here, decisions stay
//! neutral out there.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{
    CachePolicy, ContractError, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
    component_id_for_unit,
};

use crate::argv::tofu_payload_argv;
use crate::env::tofu_payload_env;
use crate::kinds::TofuTaskKind;

/// Selected compile driver spelling for tofu tasks: the invoked program.
pub const TOFU_DRIVER: &str = "tofu";
/// Selected test-runner spelling for tofu tasks: tofu runs no tests.
pub const TOFU_RUNNER: &str = "none";
/// Tofu test-runner profile name: tofu has no profiles in v1.
pub const TOFU_PROFILE: &str = "default";

/// One derived tofu task group: a root, a kind, a configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TofuTaskGroup {
    /// Normalized configured root (`""` for the repository root).
    pub root: String,
    /// Task kind.
    pub kind: TofuTaskKind,
    /// Configuration name (`default`; tofu has no configurations in v1).
    pub configuration: String,
    /// No actionable target (empty fmt scope); emit no command.
    pub no_targets: bool,
}

/// Bijective ASCII key for the exact normalized root UTF-8 bytes.
#[must_use]
pub fn key_for_root(root: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut key = String::from("dir-");
    for byte in root.bytes() {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 15)]));
    }
    key
}

/// Decode a canonical exact-root key; malformed or legacy keys fail closed.
/// # Errors
/// Returns an identity error for noncanonical keys or paths.
pub fn root_for_key(key: &str) -> Result<String, ContractError> {
    let bad = || ContractError::identity("tofu_root_key", "noncanonical_root_key");
    let hex = key.strip_prefix("dir-").ok_or_else(bad)?;
    if hex.len() % 2 != 0 || !velnor_actions_contract::ids::is_lower_hex(hex) {
        return Err(bad());
    }
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| {
                if byte <= b'9' {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect::<Vec<_>>();
    let root = String::from_utf8(bytes).map_err(|_| bad())?;
    validate_normalized_root(&root)?;
    Ok(root)
}

/// Validate an exact normalized repository-relative root.
/// # Errors
/// Rejects normalization aliases, traversal, absolute paths, and controls.
pub fn validate_normalized_root(root: &str) -> Result<(), ContractError> {
    if !root.is_empty() {
        velnor_actions_contract::config::Utf8RepoRelDir::parse(root)
            .map_err(|_| ContractError::identity("tofu_root", "invalid_normalized_root"))?;
        if root == "." {
            return Err(ContractError::identity(
                "tofu_root",
                "invalid_normalized_root",
            ));
        }
    }
    Ok(())
}

/// Display root for a normalized tofu root: `""` renders as `.`.
#[must_use]
pub fn display_for_root(root: &str) -> String {
    if root.is_empty() {
        ".".to_owned()
    } else {
        root.to_owned()
    }
}

/// Task ID for one root/kind/configuration triple.
///
/// The single authoritative constructor: keys derive from normalized
/// tofu roots (never Cargo stand-ins) and IDs flow through the
/// existing stack grammar.
///
/// # Errors
///
/// Returns [`ContractError`] for keys, kinds, or configurations
/// outside the task-ID grammar.
pub fn task_id_for_root(
    root: &str,
    kind: TofuTaskKind,
    configuration: &str,
) -> Result<String, ContractError> {
    validate_normalized_root(root)?;
    velnor_actions_contract::task_id_for_stack(
        crate::STACK_ID,
        &key_for_root(root),
        kind.as_str(),
        configuration,
        None,
    )
}

/// Convert one tofu task group into its neutral proposal.
///
/// Copies adapter-known fields verbatim (ids, edges, drivers as
/// spellings) and precomputes the payload, environment, component,
/// and project root the pipeline needs. `Validate` depends on the
/// same-root `Init`; `Fmt` is independent.
///
/// # Errors
///
/// Returns [`ContractError`] when the triple falls outside the
/// task-ID grammar, or when the fixed payload rejects a
/// leading-dash root.
pub fn propose_task(group: &TofuTaskGroup) -> Result<ProposedTask, ContractError> {
    validate_normalized_root(&group.root)?;
    let key = key_for_root(&group.root);
    let unit_path = display_for_root(&group.root);
    let project_root = unit_path.clone();
    let task_id = task_id_for_root(&group.root, group.kind, &group.configuration)?;
    let environment: BTreeMap<String, String> = tofu_payload_env(group.kind)
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let depends_on = match group.kind {
        TofuTaskKind::Validate => vec![task_id_for_root(
            &group.root,
            TofuTaskKind::InitForValidate,
            &group.configuration,
        )?],
        TofuTaskKind::Fmt | TofuTaskKind::InitForValidate => Vec::new(),
    };
    Ok(ProposedTask {
        task_id,
        stack_id: crate::STACK_ID.to_owned(),
        component_id: component_id_for_unit(&key, &unit_path),
        task_kind: group.kind.as_str().to_owned(),
        configuration: group.configuration.clone(),
        depends_on,
        gated_by: Vec::new(),
        reads: vec![unit_path.clone()],
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: resource_class_for_kind(group.kind),
            cpu_milli: None,
            memory_mb: None,
            needs_network: group.kind == TofuTaskKind::InitForValidate,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            // Tofu task-result reuse stays disabled at the gate
            // (`tofu_reuse_disabled`), so init/validate proposals say
            // false to match the enforced state; fmt stays true (its
            // reuse status is unspecified).
            allow_task_reuse: group.kind == TofuTaskKind::Fmt,
        },
        identity: IdentityInputs {
            unit_id: key.clone(),
            unit_key: key,
            unit_path,
            project_root,
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: TOFU_DRIVER.to_owned(),
            test_runner: TOFU_RUNNER.to_owned(),
            environment,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: tofu_payload_argv(group.kind, &group.root)?,
        display_name: display_for_root(&group.root),
        uses_clock: false,
        uses_random: false,
        no_targets: group.no_targets,
        runner_profile: TOFU_PROFILE.to_owned(),
    })
}

/// Resource class for one tofu task kind.
#[must_use]
pub fn resource_class_for_kind(kind: TofuTaskKind) -> ResourceClass {
    match kind {
        TofuTaskKind::InitForValidate => ResourceClass::Network,
        TofuTaskKind::Fmt | TofuTaskKind::Validate => ResourceClass::Lightweight,
    }
}

/// In-crate obligation order rank: format, init, validate.
///
/// Unknown kinds sort last; validated proposals never carry them.
#[must_use]
pub fn task_kind_rank(kind: &str) -> u32 {
    match kind {
        "fmt" => 0,
        "init" => 1,
        "validate" => 2,
        _ => u32::MAX,
    }
}

/// Human step base name for one kind.
///
/// `fmt_name` carries the renderer's shared Format name so this table
/// never duplicates it; unknown kinds echo (validated proposals never
/// carry them).
#[must_use]
pub fn step_base_name<'a>(kind: &'a str, fmt_name: &'a str) -> &'a str {
    match kind {
        "fmt" => fmt_name,
        "init" => "Init for validate",
        "validate" => "Validate",
        _ => kind,
    }
}

/// Kind words in fixed plan order: `(kind spelling, display word)`.
pub const KIND_DISPLAY_WORDS: [(&str, &str); 3] = [
    ("fmt", "format check"),
    ("init", "init for validate"),
    ("validate", "validate"),
];

/// Whether `kind` is the tofu init-for-validate gate.
///
/// There is intentionally no `is_fmt_kind`: `fmt` is shared with
/// rust, so callers dispatch it by stack, never by kind alone.
#[must_use]
pub fn is_init_kind(kind: &str) -> bool {
    kind == TofuTaskKind::InitForValidate.as_str()
}

/// Whether `kind` is the tofu validate gate.
#[must_use]
pub fn is_validate_kind(kind: &str) -> bool {
    kind == TofuTaskKind::Validate.as_str()
}

/// Fixed payload env for one kind spelling; bogus spellings map empty.
#[must_use]
pub fn payload_env_for_kind(kind: &str) -> Vec<(OsString, OsString)> {
    match TofuTaskKind::parse(kind) {
        Ok(parsed) => tofu_payload_env(parsed),
        Err(_) => Vec::new(),
    }
}
