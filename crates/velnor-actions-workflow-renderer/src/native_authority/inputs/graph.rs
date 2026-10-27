// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, invalid,
    registered::RegisteredSources, task_ids::TaskIds, tool_needs,
};
use std::collections::BTreeSet;

pub(in crate::native_authority) fn validate_graph(
    input: &NativeAuthorityInput<'_>,
) -> Result<(Vec<String>, Vec<String>), NativeAuthorityError> {
    let mut registered = RegisteredSources::new(input)?;
    let tasks = TaskIds::new(input);
    let mut ids = BTreeSet::new();
    let mut required = Vec::new();
    let mut tofu = BTreeSet::new();
    for fact in input.logical_jobs {
        velnor_actions_contract::validate_job_id(&fact.id)?;
        if matches!(
            fact.id.as_str(),
            crate::render::PLAN_JOB_ID
                | crate::render::FINAL_JOB_ID
                | crate::native_job_builders::LINT_JOB_ID
                | "required-gate"
        ) || !ids.insert(fact.id.as_str())
        {
            return Err(invalid("native_graph_reserved_or_duplicate_job"));
        }
        if matches!(&fact.source, LogicalJobSource::NamedCheck { .. }) {
            fact.check_runner
                .as_ref()
                .ok_or_else(|| invalid("native_check_job_identity"))?
                .validate("native_authority", "check_runner")?;
        } else {
            crate::guard::validate_runs_on(&fact.runs_on)?;
        }
        let is_required = match &fact.source {
            LogicalJobSource::Task(task) => {
                tasks.validate(fact, task, input, &mut tofu)?;
                true
            }
            _ => registered.admit(fact, input)?,
        };
        if is_required {
            required.push(fact.id.clone());
        }
    }
    registered.finish(input)?;
    for check in input
        .config
        .checks
        .iter()
        .map(|check| &check.id)
        .chain(input.config.native_checks.iter().map(|check| &check.id))
    {
        if !ids.contains(format!("check-{check}").as_str()) {
            return Err(invalid("native_graph_missing_configured_check"));
        }
    }
    tool_needs::validate(input)?;
    Ok((required, tofu.into_iter().collect()))
}
