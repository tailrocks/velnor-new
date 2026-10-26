// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{RenderError, Yaml};
use velnor_actions_contract::{
    JobTimeout,
    config::{PUBLICATION_CLEANUP_HEADROOM_MINUTES, PUBLICATION_PREPARATION_STEP_DEFAULT_MINUTES},
};

pub(super) fn apply_preparation_budget(
    fields: &mut [(String, Yaml)],
    steps: &mut [Yaml],
    runtime: JobTimeout,
) -> Result<(), RenderError> {
    let mut preparation_minutes = 0_u32;
    for step in steps {
        let Yaml::Map(entries) = step else {
            return Err(invalid("publication_preparation_step_not_map"));
        };
        let timeout_positions = entries
            .iter()
            .enumerate()
            .filter_map(|(index, (key, _))| (key == "timeout-minutes").then_some(index))
            .collect::<Vec<_>>();
        if timeout_positions.len() > 1 {
            return Err(invalid("publication_preparation_timeout_duplicate"));
        }
        let timeout = if let Some(index) = timeout_positions.first().copied() {
            match entries.get(index).map(|(_, value)| value) {
                Some(Yaml::Int(minutes)) if (1..=360).contains(minutes) => u32::try_from(*minutes)
                    .map_err(|_| invalid("publication_preparation_timeout_invalid"))?,
                _ => return Err(invalid("publication_preparation_timeout_invalid")),
            }
        } else {
            let minutes = u32::from(PUBLICATION_PREPARATION_STEP_DEFAULT_MINUTES);
            entries.push(("timeout-minutes".to_owned(), Yaml::Int(i64::from(minutes))));
            minutes
        };
        preparation_minutes = preparation_minutes
            .checked_add(timeout)
            .ok_or_else(|| invalid("publication_preparation_budget_overflow"))?;
    }

    let job_minutes = preparation_minutes
        .checked_add(u32::from(runtime.minutes()))
        .and_then(|minutes| minutes.checked_add(u32::from(PUBLICATION_CLEANUP_HEADROOM_MINUTES)))
        .ok_or_else(|| invalid("publication_job_budget_overflow"))?;
    let timeout = JobTimeout::try_from(job_minutes).map_err(RenderError::Contract)?;
    let timeout_positions = fields
        .iter_mut()
        .enumerate()
        .filter_map(|(index, (key, _))| (key == "timeout-minutes").then_some(index))
        .collect::<Vec<_>>();
    if timeout_positions.len() != 1 {
        return Err(invalid("publication_job_timeout_missing_or_duplicate"));
    }
    fields[timeout_positions[0]].1 = Yaml::Int(i64::from(timeout.minutes()));
    Ok(())
}

fn invalid(code: &str) -> RenderError {
    RenderError::InvalidWorkflow(code.to_owned())
}
