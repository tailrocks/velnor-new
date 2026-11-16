// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, Job, RenderError, StepKind, TaskExecutionManifestEntryV1, TaskFactorContext,
};

type NativeTasks = BTreeMap<String, TaskExecutionManifestEntryV1>;

pub(super) fn lower<'a>(
    jobs: &'a BTreeMap<String, Job>,
    context: &TaskFactorContext<'_>,
) -> Result<(std::borrow::Cow<'a, BTreeMap<String, Job>>, NativeTasks), RenderError> {
    if !jobs.values().any(|job| {
        job.steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::TofuTaskExecution { .. }))
    }) {
        return Ok((std::borrow::Cow::Borrowed(jobs), BTreeMap::new()));
    }
    let mut lowered = jobs.clone();
    let mut records = BTreeMap::new();
    for (job_id, job) in &mut lowered {
        for index in 0..job.steps.len() {
            let StepKind::TofuTaskExecution { execution } = &job.steps[index].kind else {
                continue;
            };
            execution.validate().map_err(RenderError::Contract)?;
            if execution.report_helper_version != context.report_helper_version {
                return Err(RenderError::InvalidWorkflow(format!(
                    "native_tofu_helper_version_mismatch:{job_id}"
                )));
            }
            super::validate_task_checkout(
                job_id,
                job,
                index,
                context.checkout_uses,
                context.report_helper_version,
                context.controller,
            )?;
            let record = execution.as_ref().clone();
            if records
                .insert(record.task_id.clone(), record.clone())
                .is_some()
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "native_tofu_task_not_unique:{}",
                    record.task_id
                )));
            }
            let mut env = record.env;
            env.extend([
                ("VELNOR_TASK_ID".to_owned(), record.task_id),
                ("VELNOR_TASK_DIGEST".to_owned(), record.task_digest),
                ("VELNOR_MATRIX_ID".to_owned(), record.matrix_id),
                ("VELNOR_MATRIX_KEY".to_owned(), record.matrix_key),
            ]);
            let joined = crate::join_argv_for_run(&record.argv)?;
            let run = crate::plan_report::report_wrapper_argv(
                &joined,
                &crate::plan_report::helper_path_for_version(&record.report_helper_version),
            );
            job.steps[index].kind = crate::shell_step(&job.steps[index].name, run, env)?.kind;
        }
    }
    Ok((std::borrow::Cow::Owned(lowered), records))
}

pub(super) fn extend_manifest(
    manifest: &mut NativeTasks,
    native: NativeTasks,
) -> Result<(), RenderError> {
    for (key, record) in native {
        if manifest.insert(key.clone(), record).is_some() {
            return Err(RenderError::InvalidWorkflow(format!(
                "native_tofu_task_not_unique:{key}"
            )));
        }
    }
    Ok(())
}
