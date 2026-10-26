// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::step_identity::{
    MBX_WORKSPACE_CLEAN_CONDITION, StepId, StepRole, TOFU_PROVIDER_ADMISSION_USES,
    verifier_action_role,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

mod task_execution;
pub(super) use task_execution::{TaskExecutionValidation, validate_task_execution};

pub const MAX_TASK_EXECUTION_ARGV: usize = 512;

pub const MAX_TASK_EXECUTION_ENV: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<StepId>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<StepRole>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parallel: bool,

    #[serde(flatten)]
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    Action {
        uses: String,

        #[serde(default)]
        with: std::collections::BTreeMap<String, String>,

        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },

    Shell {
        run: Vec<String>,

        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },

    TaskExecution {
        argv: Vec<String>,

        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,

        task_id: String,

        task_digest: String,

        toolchain_inputs: crate::cachekey::ToolchainInputs,

        matrix_id: String,

        matrix_key: String,

        report_helper_version: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        matrix_max_parallel: Option<u32>,
    },

    NativeCheckExecution {
        check_id: String,

        operation: crate::config::NativeHomebrewOperation,

        task_id: String,

        sdk_version: String,
    },

    Internal {
        operation: String,

        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        env: std::collections::BTreeMap<String, String>,
    },
}

impl Step {
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
        if matches!(&self.kind, StepKind::TaskExecution { .. })
            && (self.id.is_some() || self.role.is_some() || self.condition.is_some())
        {
            return Err(ContractError::identity(
                "step.task",
                format!("task_execution_authority_mismatch:{job}"),
            ));
        }
        self.validate_execution_authority(job)?;
        if let StepKind::Action { uses, .. } = &self.kind
            && let Some(expected) = verifier_action_role(uses)
            && self.role != Some(expected)
        {
            return Err(ContractError::identity(
                "step.role",
                format!("verifier_action_role_mismatch:{job}:{expected:?}"),
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
    fn validate_execution_authority(&self, job: &str) -> Result<(), ContractError> {
        if matches!(&self.kind, StepKind::NativeCheckExecution { .. })
            && (self.id.is_some()
                || self.role.is_some()
                || self.condition.is_some()
                || self.parallel)
        {
            return Err(ContractError::identity(
                "step.native_check",
                format!("native_check_authority_mismatch:{job}"),
            ));
        }

        if self.parallel
            && (self.id.is_some()
                || self.role.is_some()
                || matches!(&self.kind, StepKind::Internal { .. }))
        {
            return Err(ContractError::identity(
                "step.parallel",
                format!("parallel_step_authority:{job}"),
            ));
        }
        Ok(())
    }
}

fn validate_id_kind(id: Option<StepId>, kind: &StepKind, job: &str) -> Result<(), ContractError> {
    let Some(id) = id else { return Ok(()) };
    let valid = match id {
        StepId::Plan => {
            matches!(kind, StepKind::Internal { operation, .. } if operation == "plan-v1")
        }
        StepId::ToolsCacheIdentity => super::step_identity::valid_tools_cache_identity(kind),
        StepId::TofuProviders => {
            matches!(kind, StepKind::Action { uses, .. } if uses == TOFU_PROVIDER_ADMISSION_USES)
        }
        StepId::MbxReady
        | StepId::CacheKeys
        | StepId::SourcesFetch
        | StepId::MbxBundleKey
        | StepId::MbxBundleExport
        | StepId::FamilyKeys
        | StepId::FamilySizeBunInstall
        | StepId::FamilySizePlaywrightBrowsers
        | StepId::FamilySizeDbtPackages
        | StepId::FamilySizePythonVenv
        | StepId::FamilySizeAnsibleCollections => {
            matches!(kind, StepKind::Shell { .. })
        }
        StepId::SourcesRestore | StepId::MbxBundleRestore => {
            matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/restore@"))
        }
        StepId::DepsToken => {
            matches!(kind, StepKind::Action { uses, .. } if uses.starts_with(super::step_identity::PRIVATE_TOKEN_MINT_ACTION_PREFIX))
        }
        StepId::ToolsRestore => {
            matches!(kind, StepKind::Action { uses, .. } if uses == super::step_identity::TOOLS_CACHE_RESTORE_USES)
        }
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
        StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version,
            matrix_max_parallel,
        } => validate_task_execution(&TaskExecutionValidation {
            argv,
            env,
            task_id,
            task_digest,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version,
            matrix_max_parallel: *matrix_max_parallel,
            job,
        })?,
        StepKind::NativeCheckExecution {
            check_id,
            task_id,
            sdk_version,
            ..
        } => {
            validate_native_check_kind(check_id, task_id, sdk_version)?;
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

fn validate_native_check_kind(
    check_id: &str,
    task_id: &str,
    sdk_version: &str,
) -> Result<(), ContractError> {
    if check_id.is_empty()
        || check_id.len() > 128
        || !check_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ContractError::identity(
            "step.native_check",
            "invalid_check_id",
        ));
    }
    crate::ids::validate_task_id(task_id)
        .map_err(|_| ContractError::identity("step.native_check", "invalid_task_id"))?;
    if sdk_version.is_empty()
        || sdk_version
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')))
    {
        return Err(ContractError::identity(
            "step.native_check",
            "invalid_sdk_version",
        ));
    }
    Ok(())
}
