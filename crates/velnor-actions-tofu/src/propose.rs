use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{
    CachePolicy, ContractError, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
    component_id_for_unit,
};

use crate::argv::tofu_payload_argv;
use crate::env::tofu_payload_env;
use crate::kinds::TofuTaskKind;

pub const TOFU_DRIVER: &str = "tofu";
pub const TOFU_RUNNER: &str = "none";
pub const TOFU_PROFILE: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TofuTaskGroup {
    pub root: String,
    pub kind: TofuTaskKind,
    pub configuration: String,
    pub no_targets: bool,
}

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

#[must_use]
pub fn display_for_root(root: &str) -> String {
    if root.is_empty() {
        ".".to_owned()
    } else {
        root.to_owned()
    }
}

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

#[must_use]
pub fn resource_class_for_kind(kind: TofuTaskKind) -> ResourceClass {
    match kind {
        TofuTaskKind::InitForValidate => ResourceClass::Network,
        TofuTaskKind::Fmt | TofuTaskKind::Validate => ResourceClass::Lightweight,
    }
}

#[must_use]
pub fn task_kind_rank(kind: &str) -> u32 {
    match kind {
        "fmt" => 0,
        "init" => 1,
        "validate" => 2,
        _ => u32::MAX,
    }
}

#[must_use]
pub fn step_base_name<'a>(kind: &'a str, fmt_name: &'a str) -> &'a str {
    match kind {
        "fmt" => fmt_name,
        "init" => "Init for validate",
        "validate" => "Validate",
        _ => kind,
    }
}

pub const KIND_DISPLAY_WORDS: [(&str, &str); 3] = [
    ("fmt", "format check"),
    ("init", "init for validate"),
    ("validate", "validate"),
];

#[must_use]
pub fn is_init_kind(kind: &str) -> bool {
    kind == TofuTaskKind::InitForValidate.as_str()
}

#[must_use]
pub fn is_validate_kind(kind: &str) -> bool {
    kind == TofuTaskKind::Validate.as_str()
}

#[must_use]
pub fn payload_env_for_kind(kind: &str) -> Vec<(OsString, OsString)> {
    match TofuTaskKind::parse(kind) {
        Ok(parsed) => tofu_payload_env(parsed),
        Err(_) => Vec::new(),
    }
}
