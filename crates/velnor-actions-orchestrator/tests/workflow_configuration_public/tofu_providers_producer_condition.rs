// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::{
    DEPENDENCY_SEED_JOB_CONDITION, validate_tofu_providers_producer, validate_tools_producer,
};
use velnor_actions_contract::{Job, WorkflowPolicy};

use super::tofu_providers_producers::{finalized, producers, tofu_repo};
use super::tools_producers::{finalized as tools_finalized, is_tools_producer};

pub(super) fn survival_condition() -> String {
    format!("always() && ({DEPENDENCY_SEED_JOB_CONDITION})")
}

pub(super) fn gate_or_survival(job: &Job) -> bool {
    let condition = job.condition.as_deref();
    condition == Some(DEPENDENCY_SEED_JOB_CONDITION) || condition == Some(&survival_condition())
}

fn with_condition(job: &Job, condition: Option<String>) -> Job {
    Job {
        condition,
        ..job.clone()
    }
}

#[test]
fn tofu_providers_validator_accepts_the_gate_and_the_survival_form_only()
-> Result<(), Box<dyn Error>> {
    let jobs = finalized(&tofu_repo(&["infra"])?)?;
    let (id, producer) = *producers(&jobs).first().ok_or("no producer")?;
    for accepted in [
        DEPENDENCY_SEED_JOB_CONDITION.to_owned(),
        survival_condition(),
    ] {
        validate_tofu_providers_producer(&with_condition(producer, Some(accepted)), id)?;
    }
    let dispatch = DEPENDENCY_SEED_JOB_CONDITION.replace("== 'push'", "== 'workflow_dispatch'");
    let unprotected = DEPENDENCY_SEED_JOB_CONDITION.replace(" && github.ref_protected == true", "");
    for rejected in [
        Some("always()".to_owned()),
        Some(format!("{} || true", survival_condition())),
        Some(format!("always() && ({unprotected})")),
        Some(format!("always() && ({dispatch})")),
        Some(format!("always() || ({DEPENDENCY_SEED_JOB_CONDITION})")),
        Some(format!("always() && {DEPENDENCY_SEED_JOB_CONDITION}")),
        None,
    ] {
        assert!(
            validate_tofu_providers_producer(&with_condition(producer, rejected.clone()), id)
                .is_err(),
            "accepted {rejected:?}"
        );
    }
    Ok(())
}

#[test]
fn tools_validator_rejects_the_survival_form() -> Result<(), Box<dyn Error>> {
    let (_, jobs) = tools_finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let (id, producer) = jobs
        .iter()
        .find(|(_, job)| is_tools_producer(job))
        .ok_or("no tools producer")?;
    validate_tools_producer(producer, id)?;
    let staged = with_condition(producer, Some(survival_condition()));
    assert!(validate_tools_producer(&staged, id).is_err());
    Ok(())
}
