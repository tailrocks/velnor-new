// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::CrateJobInputs;
use crate::OrchestratorError;
use velnor_actions_contract::{CrateJob, Step};

pub(super) fn obligation_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(model.obligations.len());
    for (index, obligation) in model.obligations.iter().enumerate() {
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        let cap = (index == 0).then_some(input.max_parallel_jobs);
        steps.push(crate::matrix_step::obligation_step(
            obligation,
            input.catalog,
            &downstream,
            cap,
            input.helper_version,
            None,
        )?);
    }
    Ok(steps)
}
