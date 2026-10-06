//! Writer-election cases for save-step presentation names.

use std::collections::BTreeMap;
use velnor_actions_contract::StepRole;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::elect_cache_writers;

use super::impl_renderer_p08_elect::keyed_job;

#[test]
fn tools_cache_election_ignores_save_presentation_name() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([("plan".to_owned(), keyed_job("actionlint@1.7.12")?)]);
    elect_cache_writers(&mut jobs)?;
    let save = jobs
        .get_mut("plan")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_job_missing".to_owned()))?
        .steps
        .iter_mut()
        .find(|step| step.role == Some(StepRole::ToolsCacheSave))
        .ok_or_else(|| RenderError::InvalidWorkflow("test_save_missing".to_owned()))?;
    save.name = "Store Mise payload".to_owned();

    elect_cache_writers(&mut jobs)?;
    let saves = jobs["plan"]
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .count();
    assert_eq!(saves, 1);
    Ok(())
}
