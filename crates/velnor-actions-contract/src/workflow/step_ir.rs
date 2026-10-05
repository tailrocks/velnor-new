//! Workflow step payload types.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::errors::ContractError;

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Step name.
    pub name: String,
    /// Run condition (`if`), serialized by the workflow renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Continue the job when this step fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_on_error: Option<bool>,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}

/// Typed purpose of a renderer-owned MBX process-group shell step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MbxProcessGroupOperation {
    /// Import one restored MBX object bundle.
    Import,
    /// Export the job-owned MBX object bundle.
    Export,
}

/// Opaque authority to render one of the two fixed MBX cache operations.
///
/// The payload contains no shell text. The renderer derives the command
/// body from `operation`, so callers cannot attach arbitrary Bash to this
/// marker. Deserialization is intentionally unavailable through `StepKind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MbxProcessGroupShell {
    operation: MbxProcessGroupOperation,
    env: BTreeMap<String, String>,
}

impl MbxProcessGroupShell {
    /// Build the fixed MBX import operation payload.
    #[must_use]
    pub fn import(env: BTreeMap<String, String>) -> Self {
        Self {
            operation: MbxProcessGroupOperation::Import,
            env,
        }
    }

    /// Build the fixed MBX export operation payload.
    #[must_use]
    pub fn export(env: BTreeMap<String, String>) -> Self {
        Self {
            operation: MbxProcessGroupOperation::Export,
            env,
        }
    }

    /// Return the fixed operation selected by the payload constructor.
    #[must_use]
    pub const fn operation(&self) -> MbxProcessGroupOperation {
        self.operation
    }

    /// Return the immutable operation environment.
    #[must_use]
    pub const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }
}

/// Step payload variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pinned GitHub Action step.
    Action {
        /// Full-SHA `uses` reference.
        uses: String,
        /// Action inputs.
        #[serde(default)]
        with: BTreeMap<String, String>,
        /// Step environment (applies to main and post phases alike).
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Opaque renderer-owned Bash step for one MBX process group.
    ///
    /// The payload carries no command text and cannot be created from
    /// serialized workflow input. The renderer revalidates its operation,
    /// environment, and outer step fields before deriving the fixed command.
    #[serde(skip_deserializing)]
    MbxProcessGroupShell(MbxProcessGroupShell),
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
    },
}

impl Step {
    /// Validate one step payload.
    pub(super) fn validate(&self, job: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity(
                "step.name",
                format!("empty_name:{job}"),
            ));
        }
        if let Some(condition) = &self.condition
            && (condition.trim().is_empty() || condition.bytes().any(|b| b == b'\n' || b == b'\r'))
        {
            return Err(ContractError::identity(
                "step.condition",
                format!("bad_condition:{job}"),
            ));
        }
        match &self.kind {
            StepKind::Action { uses, .. } => {
                if uses.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.uses",
                        format!("empty_uses:{job}"),
                    ));
                }
            }
            StepKind::Shell { run, .. } => {
                if run.is_empty() || run.iter().any(|arg| arg.trim().is_empty()) {
                    return Err(ContractError::identity(
                        "step.run",
                        format!("bad_argv:{job}"),
                    ));
                }
            }
            StepKind::MbxProcessGroupShell(payload) => {
                if payload.env().iter().any(|(key, value)| {
                    key.is_empty()
                        || !key.bytes().all(|byte| {
                            byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                        })
                        || value.chars().any(|ch| matches!(ch, '\0' | '\n' | '\r'))
                }) {
                    return Err(ContractError::identity(
                        "step.mbx_process_group.env",
                        format!("bad_env:{job}"),
                    ));
                }
            }
            StepKind::Internal { operation } => {
                if operation.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.operation",
                        format!("empty_operation:{job}"),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Step, StepKind};

    #[test]
    fn serialized_process_group_marker_cannot_be_deserialized() {
        let forged = serde_json::json!({
            "kind": "mbx_process_group_shell",
            "operation": "import",
            "env": {},
            "run": ["bash", "-c", "arbitrary command"]
        });
        assert!(serde_json::from_value::<StepKind>(forged).is_err());

        let forged_step = serde_json::json!({
            "name": "Import MBX single bundle",
            "kind": "mbx_process_group_shell",
            "operation": "import",
            "env": {},
            "run": ["bash", "-c", "arbitrary command"]
        });
        assert!(serde_json::from_value::<Step>(forged_step).is_err());
    }
}
