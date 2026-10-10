use crate::errors::ContractError;

use super::TaskExecutionValidation;

pub(super) fn validate_task_and_matrix(
    input: &TaskExecutionValidation<'_>,
) -> Result<(), ContractError> {
    crate::ids::validate_task_id(input.task_id)?;
    crate::validate_digest(input.task_digest)?;
    let toolchain_id = crate::cachekey::toolchain_id(input.toolchain_inputs)?;
    if super::super::super::crate_job::task_digest_for_execution(
        input.task_id,
        input.argv,
        &toolchain_id,
    )? != input.task_digest
    {
        return Err(ContractError::identity(
            "step.task.identity",
            format!("task_digest_mismatch:{}", input.job),
        ));
    }
    crate::ids::validate_id(input.matrix_id)?;
    crate::ids::validate_matrix_key(input.matrix_key)?;
    if !input
        .matrix_id
        .ends_with(&format!("|task:{}", input.task_id))
        || crate::ids::matrix_key_for_id(input.matrix_id)? != input.matrix_key
    {
        return Err(ContractError::identity(
            "step.task.identity",
            format!("matrix_identity_mismatch:{}", input.job),
        ));
    }
    Ok(())
}

pub(super) fn validate_report_and_matrix_cap(
    input: &TaskExecutionValidation<'_>,
) -> Result<(), ContractError> {
    if !super::valid_pinned_version(input.report_helper_version)
        || input.report_helper_version.contains('+')
    {
        return Err(ContractError::identity(
            "step.task.report",
            format!("bad_helper_version:{}", input.job),
        ));
    }
    if input.matrix_max_parallel.is_some_and(|max| max == 0) {
        return Err(ContractError::identity(
            "step.task.matrix",
            format!("bad_matrix_cap:{}", input.job),
        ));
    }
    Ok(())
}
