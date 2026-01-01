//! Existing tools-cache saves must agree with the global writer election.

use std::collections::BTreeMap;

use super::impl_renderer_fixtures::{LABEL, mise};
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{
    ToolsCacheInputs, ToolsCachePayload, elect_cache_writers,
};

const TOOL: &str = "actionlint@1.7.12";

fn keyed_job() -> Result<Job, RenderError> {
    let tool_specs = [TOOL.to_owned()];
    let payload = ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: LABEL,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &mise(),
        tool_specs: &tool_specs,
        rustup_toolchain: None,
        rustup_components: &[],
    })?;
    Ok(Job {
        display_name: "Keyed".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![payload.runtime_prelude_step()?, payload.restore_step()?],
    })
}

fn paired_jobs() -> Result<BTreeMap<String, Job>, RenderError> {
    Ok(BTreeMap::from([
        ("plan".to_owned(), keyed_job()?),
        ("rust-b".to_owned(), keyed_job()?),
    ]))
}

fn canonical_save() -> Result<Step, RenderError> {
    let mut jobs = BTreeMap::from([("plan".to_owned(), keyed_job()?)]);
    elect_cache_writers(&mut jobs)?;
    jobs.get("plan")
        .and_then(|job| {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::ToolsCacheSave))
        })
        .cloned()
        .ok_or_else(|| RenderError::InvalidWorkflow("test_save_missing".to_owned()))
}

#[derive(Clone, Copy)]
enum SaveMutation {
    Key,
    Path,
    Gate,
}

fn mutated_save(save: &Step, mutation: SaveMutation) -> Result<Step, RenderError> {
    let mut mutated = save.clone();
    match mutation {
        SaveMutation::Key | SaveMutation::Path => {
            let StepKind::Action { with, .. } = &mut mutated.kind else {
                return Err(RenderError::InvalidWorkflow(
                    "test_save_not_action".to_owned(),
                ));
            };
            match mutation {
                SaveMutation::Key => {
                    with.insert("key".to_owned(), "velnor-invalid-key".to_owned());
                }
                SaveMutation::Path => {
                    with.insert("path".to_owned(), "${{ runner.temp }}/wrong".to_owned());
                }
                SaveMutation::Gate => {}
            }
        }
        SaveMutation::Gate => mutated.condition = Some("always()".to_owned()),
    }
    Ok(mutated)
}

fn save_index(job: &Job) -> Option<usize> {
    job.steps
        .iter()
        .position(|step| step.role == Some(StepRole::ToolsCacheSave))
}

#[test]
fn tools_cache_election_rejects_loser_saves_and_mutated_loser_shapes() -> Result<(), RenderError> {
    let canonical = canonical_save()?;
    for mutation in [
        None,
        Some(SaveMutation::Key),
        Some(SaveMutation::Path),
        Some(SaveMutation::Gate),
    ] {
        let mut jobs = paired_jobs()?;
        elect_cache_writers(&mut jobs)?;
        let save = match mutation {
            Some(value) => mutated_save(&canonical, value)?,
            None => canonical.clone(),
        };
        jobs.get_mut("rust-b")
            .ok_or_else(|| RenderError::InvalidWorkflow("test_loser_missing".to_owned()))?
            .steps
            .push(save);
        assert!(
            elect_cache_writers(&mut jobs).is_err(),
            "a non-elected job cannot keep a save, even when its payload is mutated"
        );
    }
    Ok(())
}

#[test]
fn tools_cache_election_rejects_mutated_winner_key_path_and_gate() -> Result<(), RenderError> {
    let canonical = canonical_save()?;
    for mutation in [SaveMutation::Key, SaveMutation::Path, SaveMutation::Gate] {
        let mut jobs = BTreeMap::from([("plan".to_owned(), keyed_job()?)]);
        let winner = jobs
            .get_mut("plan")
            .ok_or_else(|| RenderError::InvalidWorkflow("test_winner_missing".to_owned()))?;
        winner.steps.push(mutated_save(&canonical, mutation)?);
        assert!(
            elect_cache_writers(&mut jobs).is_err(),
            "an elected save must match the exact key, path, and gate"
        );
    }
    Ok(())
}

#[test]
fn tools_cache_election_ignores_winner_save_presentation_name() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([("plan".to_owned(), keyed_job()?)]);
    let mut save = canonical_save()?;
    save.name = "Store tools".to_owned();
    jobs.get_mut("plan")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_winner_missing".to_owned()))?
        .steps
        .push(save);

    elect_cache_writers(&mut jobs)?;
    let saves = jobs
        .get("plan")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_winner_missing".to_owned()))?
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .count();
    assert_eq!(
        saves, 1,
        "a renamed canonical save remains the single writer"
    );
    Ok(())
}

#[test]
fn tools_cache_election_rejects_orphan_and_duplicate_winner_saves() -> Result<(), RenderError> {
    let canonical = canonical_save()?;
    let orphan = Job {
        display_name: "Orphan".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![canonical.clone()],
    };
    assert!(elect_cache_writers(&mut BTreeMap::from([("orphan".to_owned(), orphan)])).is_err());

    let mut jobs = BTreeMap::from([("plan".to_owned(), keyed_job()?)]);
    elect_cache_writers(&mut jobs)?;
    let winner = jobs
        .get_mut("plan")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_winner_missing".to_owned()))?;
    let index = save_index(winner)
        .ok_or_else(|| RenderError::InvalidWorkflow("test_save_missing".to_owned()))?;
    winner.steps.push(winner.steps[index].clone());
    assert!(elect_cache_writers(&mut jobs).is_err());
    Ok(())
}
