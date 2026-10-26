// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_actions_contract::ContractError;

use crate::profile::{CompileDriver, TestRunner};
use crate::tasks::TaskKind;

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
pub struct RustTaskIdentityExtension {
    pub package_id: String,
    pub workspace_id: String,
    pub profile: String,
    pub manifest: String,
    pub graph_digest: String,
    pub targets: Vec<String>,
    pub features: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub all_features: bool,
    pub target: String,
    pub driver: String,
    #[serde(skip_serializing)]
    pub compile_driver: CompileDriver,
    #[serde(skip_serializing)]
    pub test_runner: TestRunner,
    pub config_digest: String,
    pub nextest_digest: Option<String>,
    pub nextest_state: SlotState,
    #[serde(skip_serializing)]
    pub nextest_slot: DigestSlot,
    pub kind: String,
    #[serde(skip_serializing)]
    pub task_kind: TaskKind,
    pub undeclared_reads: bool,
    pub lock_digest: Option<String>,
    pub lock_state: SlotState,
    #[serde(skip_serializing)]
    pub lock_slot: DigestSlot,
    pub archive: Option<String>,
    pub rerun_inputs: Vec<String>,
    pub declared_inputs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    pub package_id: &'a str,
    pub workspace_id: &'a str,
    pub profile: &'a str,
    pub manifest: &'a str,
    pub graph_digest: &'a str,
    pub targets: &'a [String],
    pub features: &'a [String],
    pub all_features: bool,
    pub target: &'a str,
    pub driver: CompileDriver,
    pub runner: TestRunner,
    pub config_digest: &'a str,
    pub lock_digest: DigestSlot,
    pub nextest_digest: DigestSlot,
    pub kind: TaskKind,
    pub archive_source: Option<&'a str>,
    pub rerun_inputs: Option<&'a [String]>,
    pub has_build_script: bool,
    pub declared_inputs: &'a [String],
}

impl RustTaskIdentityExtension {
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: "rust-task-identity-v1".to_owned(),
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
        if let Some(input) = self.first_blocking_input() {
            return Err(ContractError::identity(
                "stack_extension",
                format!("unresolved_input:{}", input.as_str()),
            ));
        }
        Ok(())
    }

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
            all_features: inputs.all_features,
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

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a borrowed boolean"
)]
fn is_false(value: &bool) -> bool {
    !value
}

fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out.dedup();
    out
}
