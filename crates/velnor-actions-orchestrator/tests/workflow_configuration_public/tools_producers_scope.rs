// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::{DEPENDENCY_SEED_JOB_CONDITION, validate_tools_producer};
use velnor_actions_contract::{Job, WorkflowPolicy};

use super::tools_producers::{finalized, is_tools_producer};

#[test]
fn producer_validator_accepts_only_the_push_condition() -> Result<(), Box<dyn Error>> {
    let (_, jobs) = finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let (id, producer) = jobs
        .iter()
        .find(|(_, job)| is_tools_producer(job))
        .ok_or("no producer")?;
    validate_tools_producer(producer, id)?;
    let scheduled = "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
    let dispatch = DEPENDENCY_SEED_JOB_CONDITION.replace("== 'push'", "== 'workflow_dispatch'");
    for rejected in [
        Some(scheduled.to_owned()),
        Some(dispatch),
        Some(format!("{DEPENDENCY_SEED_JOB_CONDITION} || true")),
        Some(format!(
            "({DEPENDENCY_SEED_JOB_CONDITION}) || ({scheduled})"
        )),
        Some(DEPENDENCY_SEED_JOB_CONDITION.replace(" && github.ref_protected == true", "")),
        Some("github.event_name == 'push'".to_owned()),
        None,
    ] {
        let job = Job {
            condition: rejected.clone(),
            ..producer.clone()
        };
        assert!(
            validate_tools_producer(&job, id).is_err(),
            "accepted {rejected:?}"
        );
    }
    let (crate_id, crate_job) = jobs
        .iter()
        .find(|(_, job)| job.producer_kind().is_none())
        .ok_or("no non-producer job")?;
    assert!(validate_tools_producer(crate_job, crate_id).is_err());
    Ok(())
}
