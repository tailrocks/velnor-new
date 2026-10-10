//! Workflow step payloads and local validation.

use super::step_identity::{
    MBX_WORKSPACE_CLEAN_CONDITION, StepId, StepRole, TOFU_PROVIDER_ADMISSION_USES,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Step name shown in workflow logs; presentation only.
    pub name: String,
    /// Stable GitHub Actions id for output-producing steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<StepId>,
    /// Typed semantic role; never inferred from the display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<StepRole>,
    /// Run condition (`if`), serialized by the workflow renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
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
        with: std::collections::BTreeMap<String, String>,
        /// Step environment (applies to main and post phases alike).
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
        /// Fixed environment values attached by workflow expansion.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        env: std::collections::BTreeMap<String, String>,
    },
}

impl Step {
    /// Validate one step payload and its typed authority.
    pub(crate) fn validate(&self, job: &str) -> Result<(), ContractError> {
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
        if let Some(role) = self.role {
            role.validate(&self.kind, job)?;
            if role == StepRole::ToolSeed && self.condition.is_some() {
                return Err(ContractError::identity(
                    "step.role",
                    format!("tool_seed_conditional:{job}"),
                ));
            }
            if role == StepRole::MbxWorkspaceCleanup
                && self.condition.as_deref() != Some(MBX_WORKSPACE_CLEAN_CONDITION)
            {
                return Err(ContractError::identity(
                    "step.condition",
                    format!("mbx_cleanup_condition_mismatch:{job}"),
                ));
            }
            if let Some(expected) = role.required_id()
                && self.id != Some(expected)
            {
                return Err(ContractError::identity(
                    "step.id",
                    format!("role_id_mismatch:{job}:{role:?}"),
                ));
            }
        }
        if let Some(id) = self.id
            && self.role != Some(StepRole::for_id(id))
        {
            return Err(ContractError::identity(
                "step.role",
                format!("id_role_mismatch:{job}:{}", id.as_str()),
            ));
        }
        validate_id_kind(self.id, &self.kind, job)?;
        validate_kind(&self.kind, job)
    }
}
/// Require an output id to belong to its typed payload kind.
fn validate_id_kind(id: Option<StepId>, kind: &StepKind, job: &str) -> Result<(), ContractError> {
    let Some(id) = id else { return Ok(()) };
    let valid = match id {
        StepId::Plan => {
            matches!(kind, StepKind::Internal { operation, .. } if operation == "plan-v1")
        }
        StepId::PublishBaseline => {
            matches!(kind, StepKind::Internal { operation, .. } if operation == "publish-baseline-v1")
        }
        StepId::ToolsCacheIdentity => super::step_identity::valid_tools_cache_identity(kind),
        StepId::TofuProviders => {
            matches!(kind, StepKind::Action { uses, .. } if uses == TOFU_PROVIDER_ADMISSION_USES)
        }
        StepId::MbxReady => matches!(kind, StepKind::Shell { .. }),
    };
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "step.id",
            format!("id_kind_mismatch:{job}:{id:?}"),
        ))
    }
}

/// Validate the payload-specific required fields.
fn validate_kind(kind: &StepKind, job: &str) -> Result<(), ContractError> {
    match kind {
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
        StepKind::Internal { operation, env } => {
            if operation.trim().is_empty() {
                return Err(ContractError::identity(
                    "step.operation",
                    format!("empty_operation:{job}"),
                ));
            }
            if env.keys().any(|key| key.trim().is_empty()) {
                return Err(ContractError::identity(
                    "step.env",
                    format!("bad_internal_env:{job}"),
                ));
            }
        }
    }
    Ok(())
}
