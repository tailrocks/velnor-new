//! Read-only tool consumers never gain executable-cache save authority.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, StepId, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{ensure_setup_p08, validate_tool_consumers};
use velnor_actions_workflow_renderer::steps::{
    TOOLS_RESTORE_USES, TOOLS_SAVE_USES, cache_action_step, tool_payload_paths,
};

use super::impl_renderer_fixtures::*;

const CONSUMER_KEY: &str = "owned-tools";

fn consumer_job(with_save: bool) -> Result<Job, RenderError> {
    let mut restore = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        CONSUMER_KEY,
        &["owned-tools-snapshot-".to_owned()],
        &tool_payload_paths(),
    )?;
    restore.id = Some(StepId::new("velnor-tools-cache").expect("restore id"));
    let mut steps = vec![restore];
    if with_save {
        let save = cache_action_step(
            false,
            TOOLS_SAVE_USES,
            "tools",
            CONSUMER_KEY,
            &[],
            &tool_payload_paths(),
        )?;
        steps.push(save);
    }
    Ok(Job {
        cache_mode: None,
        display_name: "Tool consumer".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps,
    })
}

fn save_count(job: &Job) -> usize {
    job.steps
        .iter()
        .filter(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/cache/save@"))
        })
        .count()
}

#[test]
fn consumers_sharing_one_restore_key_remain_read_only() -> Result<(), RenderError> {
    let jobs = BTreeMap::from([
        ("rust-a".to_owned(), consumer_job(false)?),
        ("rust-b".to_owned(), consumer_job(false)?),
    ]);
    let before = jobs["rust-a"].steps.clone();
    validate_tool_consumers(&jobs, &mise(), &[])?;
    assert_eq!(jobs["rust-a"].steps, before);
    assert_eq!(save_count(&jobs["rust-a"]), 0);
    assert_eq!(save_count(&jobs["rust-b"]), 0);
    Ok(())
}

#[test]
fn renamed_preexisting_consumer_save_is_rejected() -> Result<(), RenderError> {
    let mut save = consumer_job(true)?;
    save.steps[1].name = "Renamed owned save".to_owned();
    let jobs = BTreeMap::from([("loser".to_owned(), save)]);
    let error = validate_tool_consumers(&jobs, &mise(), &[])
        .expect_err("preexisting consumer save must fail closed");
    assert!(error.to_string().contains("tool_consumer_write:loser"));
    Ok(())
}

#[test]
fn ordinary_no_tool_job_cannot_carry_an_owned_restore() -> Result<(), RenderError> {
    let mut job = consumer_job(false)?;
    let error = ensure_setup_p08(
        "bare-consumer",
        &mut job,
        &mise(),
        false,
        "x86_64-unknown-linux-gnu",
        &[],
    )
    .expect_err("owned restore must bind to a tool role");
    assert!(error.to_string().contains("tool_consumer_unbound_restore"));
    Ok(())
}
