// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_actions_contract::ContractError;

use crate::kinds::TofuTaskKind;
use crate::propose::{TOFU_DRIVER, TOFU_RUNNER};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestSlot {
    Known(String),
    AbsentProven(String),
    Unknown(String),
}

impl DigestSlot {
    #[must_use]
    pub fn as_known(&self) -> Option<&str> {
        match self {
            Self::Known(digest) => Some(digest),
            Self::AbsentProven(_) | Self::Unknown(_) => None,
        }
    }

    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    #[must_use]
    pub fn state(&self) -> SlotState {
        match self {
            Self::Known(_) => SlotState::Known,
            Self::AbsentProven(_) => SlotState::AbsentProven,
            Self::Unknown(_) => SlotState::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    Known,
    AbsentProven,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TofuTaskIdentityExtension {
    pub unit_id: String,
    pub workspace_id: String,
    pub profile: String,
    pub manifest: String,
    pub graph_digest: String,
    pub root: String,
    pub kind: String,
    #[serde(skip_serializing)]
    pub task_kind: TofuTaskKind,
    pub driver: String,
    pub config_digest: String,
    pub lock_digest: Option<String>,
    pub lock_state: SlotState,
    #[serde(skip_serializing)]
    pub lock_slot: DigestSlot,
    pub undeclared_reads: bool,
    pub declared_inputs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    pub unit_id: &'a str,
    pub workspace_id: &'a str,
    pub profile: &'a str,
    pub manifest: &'a str,
    pub graph_digest: &'a str,
    pub root: &'a str,
    pub config_digest: &'a str,
    pub lock_digest: DigestSlot,
    pub kind: TofuTaskKind,
    pub undeclared_reads: bool,
    pub declared_inputs: &'a [String],
}

impl TofuTaskIdentityExtension {
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: velnor_actions_contract::cachekey::TOFU_EXTENSION_SCHEMA.to_owned(),
            data: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }

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

    #[must_use]
    pub fn conservative_execution_required(&self) -> bool {
        self.undeclared_reads || self.lock_slot.is_unknown()
    }

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

fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out.dedup();
    out
}

pub fn provider_toolchain_entries(
    unit_key: &str,
    kind_spelling: &str,
    tofu_spec: &str,
) -> Result<Vec<String>, ContractError> {
    use velnor_actions_contract::canonical::digest_b3_typed;
    use velnor_actions_contract::canonical_json_bytes;
    crate::root_for_key(unit_key)?;
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

pub fn toolchain_inputs_for_task(
    task: &velnor_actions_contract::ProposedTask,
    specs: Vec<String>,
) -> Result<velnor_actions_contract::cachekey::ToolchainInputs, ContractError> {
    use velnor_actions_contract::cachekey::ToolchainInputs;
    crate::normalized_root_for_proposal(task)?;
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
