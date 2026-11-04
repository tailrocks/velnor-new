// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_contract::workflow::{DEPENDENCY_SEED_JOB_CONDITION, validate_tools_producer};
use velnor_actions_contract::{CacheWriter, Job, WorkflowPolicy};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::rendered_job_block;
use super::tools_producers::{finalized, is_tools_producer};
use super::tools_producers_donors::{SCHEDULED_SUITES_CONFIG, producer_ids_and_digests};
use super::workflow_configuration_fixture;

const SUITES_PATH: &str = ".github/workflows/suites.yml";

fn rendered_pair() -> Result<(String, String, String), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
    let suites = tree.get(SUITES_PATH).ok_or("suites workflow is missing")?;
    let sealed = tree
        .get(STATE_SEAL_WORKFLOW_PATH)
        .ok_or("state-seal workflow is missing")?;
    Ok((ci.to_owned(), suites.to_owned(), sealed.to_owned()))
}

#[test]
fn state_seal_workflow_holds_one_push_only_producer_per_identity_of_every_workflow()
-> Result<(), Box<dyn Error>> {
    let (ci, suites, sealed) = rendered_pair()?;
    let (ci_produced, ci_consumed) = producer_ids_and_digests(&ci);
    assert_eq!(
        ci_produced,
        BTreeSet::new(),
        "ci.yml holds a tools producer"
    );
    assert!(!ci.contains("tools-producer-"));
    let (produced, _) = producer_ids_and_digests(&sealed);
    let (_, suite_consumed) = producer_ids_and_digests(&suites);
    let suite_only: BTreeSet<_> = suite_consumed.difference(&ci_consumed).cloned().collect();
    assert_ne!(
        suite_only.len(),
        0,
        "the suite shares every identity with CI"
    );
    let expected: BTreeSet<_> = ci_consumed.union(&suite_consumed).cloned().collect();
    assert_eq!(produced, expected);
    for short in &produced {
        let id = format!("tools-producer-{short}");
        let block = rendered_job_block(&sealed, &id)?;
        let condition = block
            .lines()
            .find_map(|line| line.strip_prefix("    if: "))
            .ok_or_else(|| format!("{id} has no condition"))?;
        assert_eq!(
            condition.trim_matches('"'),
            CacheWriter::ToolsProducer.condition(),
            "{id}"
        );
        assert_eq!(block.matches("name: Save Mise tools").count(), 1, "{id}");
    }
    Ok(())
}

#[test]
fn suites_workflow_has_no_tools_producer_or_tools_save() -> Result<(), Box<dyn Error>> {
    let (_, suites, _) = rendered_pair()?;
    let (produced, consumed) = producer_ids_and_digests(&suites);
    assert_eq!(produced, BTreeSet::new());
    assert_ne!(consumed.len(), 0);
    assert!(!suites.contains("Save Mise tools"));
    assert!(!suites.contains("tools-producer-"));
    Ok(())
}

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
