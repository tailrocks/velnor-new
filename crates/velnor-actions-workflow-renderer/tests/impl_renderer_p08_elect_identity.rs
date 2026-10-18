//! Static identity boundaries for tools cache writer election.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::elect_cache_writers;

use super::impl_renderer_p08_elect::keyed_job;

#[test]
fn tools_cache_election_binds_restore_to_static_identity_digest() -> Result<(), RenderError> {
    use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;

    let mut jobs = BTreeMap::from([
        ("rust-a".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    // Same canonical restore key, but a retargeted static digest splits the
    // election: each identity elects its own writer.
    let owner = jobs
        .get_mut("rust-b")
        .and_then(|job| job.steps.first_mut())
        .ok_or_else(|| RenderError::InvalidWorkflow("test_identity_missing".to_owned()))?;
    let StepKind::Action { with, .. } = &mut owner.kind else {
        return Err(RenderError::InvalidWorkflow(
            "test_identity_not_action".to_owned(),
        ));
    };
    with.insert(TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(), "f".repeat(64));

    let key_a = restore_key(jobs.get("rust-a").ok_or_else(missing_job)?)
        .ok_or_else(|| RenderError::InvalidWorkflow("test_restore_key_missing".to_owned()))?;
    let key_b = restore_key(jobs.get("rust-b").ok_or_else(missing_job)?)
        .ok_or_else(|| RenderError::InvalidWorkflow("test_restore_key_missing".to_owned()))?;
    assert_eq!(key_a, key_b);

    elect_cache_writers(&mut jobs)?;
    for id in ["rust-a", "rust-b"] {
        let job = jobs.get(id).ok_or_else(missing_job)?;
        assert_eq!(tool_save_count(job), 1);
    }
    Ok(())
}

fn missing_job() -> RenderError {
    RenderError::InvalidWorkflow("test_job_missing".to_owned())
}

fn restore_key(job: &Job) -> Option<&str> {
    job.steps.iter().find_map(|step| {
        if step.role != Some(StepRole::ToolsCacheRestore) {
            return None;
        }
        let StepKind::Action { with, .. } = &step.kind else {
            return None;
        };
        with.get("key").map(String::as_str)
    })
}

fn tool_save_count(job: &Job) -> usize {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .count()
}
