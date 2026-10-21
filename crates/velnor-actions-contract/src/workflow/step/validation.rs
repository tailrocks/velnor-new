use super::qualification_cache::qualification_cache_id_matches;
mod qualification_cache;
use super::task_execution::{TaskExecutionValidation, validate_task_execution};
use super::{Step, StepKind};
use crate::errors::ContractError;
use crate::workflow::step_identity::{
    MBX_WORKSPACE_CLEAN_CONDITION, NORMAL_TOOLS_CACHE_EXPORT_OPERATION,
    NORMAL_TOOLS_CACHE_IMPORT_OPERATION, NORMAL_TOOLS_CACHE_STAGE_OPERATION, StepId, StepRole,
    TOFU_PROVIDER_ADMISSION_USES,
};
use qualification_cache::qualification_cache_composite_id_matches;

impl Step {
    /// Validate one step payload and its typed authority.
    pub(crate) fn validate(&self, job: &str) -> Result<(), ContractError> {
        validate_step_basics(self, job)?;
        validate_step_role(self, job)?;
        if let Some(id) = self.id
            && self.role != Some(StepRole::for_id(id))
        {
            return Err(ContractError::identity(
                "step.role",
                format!("id_role_mismatch:{job}:{}", id.as_str()),
            ));
        }
        validate_id_kind(self, job)?;
        validate_kind(&self.kind, job)
    }
}

fn validate_step_basics(step: &Step, job: &str) -> Result<(), ContractError> {
    if step.name.trim().is_empty() {
        return Err(ContractError::identity(
            "step.name",
            format!("empty_name:{job}"),
        ));
    }
    if step.condition.as_ref().is_some_and(|condition| {
        condition.trim().is_empty() || condition.bytes().any(|byte| matches!(byte, b'\n' | b'\r'))
    }) {
        return Err(ContractError::identity(
            "step.condition",
            format!("bad_condition:{job}"),
        ));
    }
    if matches!(step.kind, StepKind::TaskExecution { .. })
        && (step.id.is_some() || step.role.is_some() || step.condition.is_some())
    {
        return Err(ContractError::identity(
            "step.task",
            format!("task_execution_authority_mismatch:{job}"),
        ));
    }
    if step.parallel
        && (step.id.is_some()
            || step.role.is_some()
            || matches!(step.kind, StepKind::Internal { .. }))
    {
        return Err(ContractError::identity(
            "step.parallel",
            format!("parallel_step_authority:{job}"),
        ));
    }
    Ok(())
}

fn validate_step_role(step: &Step, job: &str) -> Result<(), ContractError> {
    let Some(role) = step.role else { return Ok(()) };
    role.validate(&step.kind, job)?;
    validate_role_condition(role, step, job)?;
    validate_role_id(role, step, job)
}

fn validate_role_condition(role: StepRole, step: &Step, job: &str) -> Result<(), ContractError> {
    if role == StepRole::ToolSeed && step.condition.is_some() {
        return Err(ContractError::identity(
            "step.role",
            format!("tool_seed_conditional:{job}"),
        ));
    }
    if role == StepRole::MbxWorkspaceCleanup
        && step.condition.as_deref() != Some(MBX_WORKSPACE_CLEAN_CONDITION)
    {
        return Err(ContractError::identity(
            "step.condition",
            format!("mbx_cleanup_condition_mismatch:{job}"),
        ));
    }
    Ok(())
}

fn validate_role_id(role: StepRole, step: &Step, job: &str) -> Result<(), ContractError> {
    if let Some(expected) = role.required_id()
        && step.id != Some(expected)
    {
        return Err(ContractError::identity(
            "step.id",
            format!("role_id_mismatch:{job}:{role:?}"),
        ));
    }
    let dynamic = matches!(
        role,
        StepRole::QualificationCacheBind
            | StepRole::QualificationCacheRestore
            | StepRole::QualificationCacheSave
            | StepRole::QualificationCacheRoots
            | StepRole::QualificationRuntimeIdentity
            | StepRole::QualificationCachePrepareComposite
            | StepRole::QualificationCacheBeforeSaveComposite
            | StepRole::QualificationCacheAfterSaveComposite
    );
    if dynamic
        && !matches!(
            step.id,
            Some(StepId::QualificationCache(_) | StepId::QualificationCacheComposite(_))
        )
    {
        return Err(ContractError::identity(
            "step.id",
            format!("qualification_cache_dynamic_id_missing:{job}:{role:?}"),
        ));
    }
    if let Some(StepId::QualificationCache(identity)) = step.id
        && !identity.belongs_to(job)
    {
        return Err(ContractError::identity(
            "step.id",
            format!("qualification_cache_owner_mismatch:{job}"),
        ));
    }
    if let Some(StepId::QualificationCacheComposite(identity)) = step.id
        && !identity.belongs_to(job)
    {
        return Err(ContractError::identity(
            "step.id",
            format!("qualification_cache_composite_owner_mismatch:{job}"),
        ));
    }
    if let Some(StepId::QualificationCacheComposite(identity)) = step.id {
        let expected = match identity.phase() {
            crate::workflow::step_id::QualificationCacheCompositePhase::Prepare => {
                StepRole::QualificationCachePrepareComposite
            }
            crate::workflow::step_id::QualificationCacheCompositePhase::BeforeSave => {
                StepRole::QualificationCacheBeforeSaveComposite
            }
            crate::workflow::step_id::QualificationCacheCompositePhase::AfterSave => {
                StepRole::QualificationCacheAfterSaveComposite
            }
        };
        if step.role != Some(expected) {
            return Err(ContractError::identity(
                "step.role",
                format!("qualification_cache_composite_role_mismatch:{job}"),
            ));
        }
    }
    Ok(())
}
/// Require an output id to belong to its typed payload kind.
fn validate_id_kind(step: &Step, job: &str) -> Result<(), ContractError> {
    let id = step.id;
    let kind = &step.kind;
    let Some(id) = id else { return Ok(()) };
    let valid = match id {
        StepId::Plan => {
            matches!(kind, StepKind::Internal { operation, .. } if operation == "plan-v1")
        }
        StepId::PublishBaseline => {
            matches!(kind, StepKind::Internal { operation, .. } if operation == "publish-baseline-v1")
        }
        StepId::ToolsCacheIdentity => {
            crate::workflow::step_identity::valid_tools_cache_identity(kind)
        }
        StepId::NormalToolsCacheStage => matches!(
            kind,
            StepKind::Internal { operation, .. } if operation == NORMAL_TOOLS_CACHE_STAGE_OPERATION
        ),
        StepId::NormalToolsCacheImport => matches!(
            kind,
            StepKind::Internal { operation, .. } if operation == NORMAL_TOOLS_CACHE_IMPORT_OPERATION
        ),
        StepId::NormalToolsCacheExport => matches!(
            kind,
            StepKind::Internal { operation, .. } if operation == NORMAL_TOOLS_CACHE_EXPORT_OPERATION
        ),
        StepId::CacheTarLaunchers => crate::workflow::step_identity::valid_cache_tar_launcher(kind),
        StepId::PreseedStage => matches!(kind, StepKind::Shell { .. }),
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
            matches!(kind, StepKind::Action { uses, .. } if uses.starts_with(crate::workflow::step_identity::PRIVATE_TOKEN_MINT_ACTION_PREFIX))
        }
        StepId::ToolsRestore => {
            matches!(kind, StepKind::Action { uses, .. } if uses == crate::workflow::step_identity::TOOLS_CACHE_RESTORE_USES)
        }
        StepId::QualificationCache(identity) => qualification_cache_id_matches(identity, kind),
        StepId::QualificationCacheComposite(identity) => {
            qualification_cache_composite_id_matches(step, identity, kind, job)?
        }
        StepId::QualificationRuntimeIdentity => matches!(
            kind,
            StepKind::Internal { operation, .. }
                if operation == "observe-qualification-cache-runtime-v1"
        ),
        StepId::QualificationCacheReceipt => matches!(
            kind,
            StepKind::Internal { operation, .. }
                if operation == "write-qualification-cache-receipt-v1"
        ),
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
