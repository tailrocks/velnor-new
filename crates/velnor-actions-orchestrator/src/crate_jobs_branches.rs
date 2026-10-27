// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{CacheMode, Job};

use super::build_jobs::reject_seed_needs;
use super::contract_problem;
use super::stage::{
    BranchJobs, is_recoverable_probe, reader_branch_condition, rewrite_recheck_step_keys,
    validate_combined_branch, writer_branch_condition, writer_branch_needs, writer_recheck_step,
};
use crate::OrchestratorError;

pub(super) fn finalize_group_branches(
    base: Job,
    branch: &super::stage::CombinedBranch,
) -> Result<(Job, Job), OrchestratorError> {
    let affected_gate = base.condition.clone();
    let base_had_probe = base.steps.iter().any(is_recoverable_probe);
    let mut reader = base.clone();
    reader.display_name = format!("{} (read)", base.display_name);
    reader.condition = Some(
        reader_branch_condition(affected_gate.as_deref())
            .map_err(|error| contract_problem(&error))?,
    );
    reader.cache_mode = Some(CacheMode::explicit_read());
    if base.steps.iter().any(|step| {
        step.id == Some(velnor_actions_contract::StepId::MbxBundleKey)
    }) {
        // Publish the key computation for the writer recheck: the recheck
        // runs before the writer's own key step, so it reads the reader's
        // outputs through `needs` instead of a later step's outputs.
        reader.outputs.insert(
            velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_JOB_OUTPUT
                .to_owned(),
            velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR.to_owned(),
        );
        reader.outputs.insert(
            velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT
                .to_owned(),
            velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_RESTORE_KEYS_EXPR
                .to_owned(),
        );
    }
    let mut writer_steps = Vec::with_capacity(base.steps.len() + 1);
    if let Some(mut recheck) = writer_recheck_step(&base.steps) {
        rewrite_recheck_step_keys(&mut recheck, &branch.reader);
        writer_steps.push(recheck);
    }
    writer_steps.extend(base.steps.clone());
    let mut writer = base;
    let writer_display = writer.display_name.clone();
    writer.display_name = format!("{writer_display} (write)");
    writer.steps = writer_steps;
    writer.needs = writer_branch_needs(&writer.needs, &branch.reader)
        .map_err(|error| contract_problem(&error))?;
    reject_seed_needs(&writer.needs, &branch.writer)?;
    writer.condition = Some(
        writer_branch_condition(affected_gate.as_deref())
            .map_err(|error| contract_problem(&error))?,
    );
    writer.cache_mode = Some(CacheMode::push_write_expression());
    validate_combined_branch(
        branch,
        affected_gate.as_deref(),
        base_had_probe,
        &BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    )
    .map_err(|error| contract_problem(&error))?;
    Ok((reader, writer))
}
