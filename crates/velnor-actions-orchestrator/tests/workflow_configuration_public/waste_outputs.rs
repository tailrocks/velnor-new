// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::StepRole;
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::mbx_repo;
use super::waste_graph::recheck_copy;

#[test]
fn generation_refuses_a_role_less_copy_of_a_restore() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let mut prepared = prepare(repo.path())?;
    finalized_jobs(&prepared)?;
    let (id, copy) = prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(_, job)| job.producer_kind().is_none())
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::CargoSourcesRestore))
                .map(|step| (id.clone(), recheck_copy(step)))
        })
        .ok_or("no sources restore in the workflow")?;
    prepared
        .workflow
        .ir
        .jobs
        .get_mut(&id)
        .ok_or("job vanished")?
        .steps
        .push(copy);
    let message = finalized_jobs(&prepared)
        .err()
        .ok_or("a repeated restore generated")?
        .to_string();
    assert!(message.contains("W4"), "{message}");
    assert!(message.contains(&id), "{message}");
    Ok(())
}
