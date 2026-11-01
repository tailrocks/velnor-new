// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::{
    DEPENDENCY_SEED_JOB_CONDITION, ProducerScope, SUITE_PRODUCER_JOB_CONDITION,
    validate_tools_producer,
};
use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::support::rendered_job_block;
use super::tools_producers::{finalized, is_tools_producer};
use super::tools_producers_donors::{SCHEDULED_SUITES_CONFIG, producer_ids_and_digests};
use super::workflow_configuration_fixture;

type ProducerConditions = Vec<(String, String, String)>;

fn producer_if_lines(workflow: &str) -> Result<ProducerConditions, Box<dyn Error>> {
    let (produced, _) = producer_ids_and_digests(workflow);
    let mut found = Vec::new();
    for short in produced {
        let id = format!("tools-producer-{short}");
        let block = rendered_job_block(workflow, &id)?;
        let condition = block
            .lines()
            .find_map(|line| line.strip_prefix("    if: "))
            .ok_or_else(|| format!("{id} has no condition"))?;
        let mut lines = block
            .lines()
            .skip_while(|line| !line.ends_with("name: Save Mise tools"));
        lines.next();
        let save = lines
            .next()
            .and_then(|line| line.trim().strip_prefix("if: "))
            .ok_or_else(|| format!("{id} has no save condition"))?;
        found.push((id, condition.to_owned(), save.to_owned()));
    }
    Ok(found)
}

#[test]
fn producer_conditions_match_their_workflow_triggers() -> Result<(), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let ci = tree
        .get(velnor_actions_workflow_renderer::render::WORKFLOW_PATH)
        .ok_or("CI workflow is missing")?;
    assert!(ci.contains("push:"));
    let ci_producers = producer_if_lines(ci)?;
    assert_ne!(ci_producers.len(), 0);
    for (id, condition, save) in &ci_producers {
        assert_eq!(condition, DEPENDENCY_SEED_JOB_CONDITION, "{id}");
        assert!(save.contains("github.event_name == 'push'"), "{id}");
    }
    let suites = tree
        .get(".github/workflows/suites.yml")
        .ok_or("suites workflow is missing")?;
    assert!(suites.contains("schedule:"));
    assert!(!suites.contains("push:") && !suites.contains("pull_request"));
    let suite_producers = producer_if_lines(suites)?;
    assert_ne!(suite_producers.len(), 0);
    for (id, condition, save) in &suite_producers {
        assert_eq!(condition, SUITE_PRODUCER_JOB_CONDITION, "{id}");
        assert!(save.contains("github.event_name == 'schedule'"), "{id}");
        assert!(!save.contains("github.event_name == 'push'"), "{id}");
    }
    Ok(())
}

#[test]
fn producer_validator_accepts_exactly_the_condition_of_its_workflow() -> Result<(), Box<dyn Error>>
{
    let (_, jobs) = finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let (id, push_producer) = jobs
        .iter()
        .find(|(_, job)| is_tools_producer(job))
        .ok_or("no producer")?;
    validate_tools_producer(push_producer, id, ProducerScope::Push)?;
    assert!(validate_tools_producer(push_producer, id, ProducerScope::Scheduled).is_err());
    let with_condition = |condition: String| Job {
        condition: Some(condition),
        ..push_producer.clone()
    };
    let scheduled = with_condition(SUITE_PRODUCER_JOB_CONDITION.to_owned());
    validate_tools_producer(&scheduled, id, ProducerScope::Scheduled)?;
    assert!(validate_tools_producer(&scheduled, id, ProducerScope::Push).is_err());
    for loose in [
        format!("{SUITE_PRODUCER_JOB_CONDITION} || true"),
        format!("({DEPENDENCY_SEED_JOB_CONDITION}) || ({SUITE_PRODUCER_JOB_CONDITION})"),
        SUITE_PRODUCER_JOB_CONDITION.replace(" && github.ref_protected == true", ""),
        SUITE_PRODUCER_JOB_CONDITION.replace("github.event_name == 'schedule' || ", ""),
    ] {
        let job = with_condition(loose.clone());
        for scope in [ProducerScope::Push, ProducerScope::Scheduled] {
            assert!(
                validate_tools_producer(&job, id, scope).is_err(),
                "{scope:?} accepted {loose}"
            );
        }
    }
    let (crate_id, crate_job) = jobs
        .iter()
        .find(|(_, job)| job.producer_kind().is_none())
        .ok_or("no non-producer job")?;
    for scope in [ProducerScope::Push, ProducerScope::Scheduled] {
        assert!(validate_tools_producer(crate_job, crate_id, scope).is_err());
    }
    Ok(())
}
