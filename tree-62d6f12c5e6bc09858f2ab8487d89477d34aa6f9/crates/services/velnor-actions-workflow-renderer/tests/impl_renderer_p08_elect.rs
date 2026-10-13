//! P08 writer-election cases: one Mise-cache saver per cache key.

use std::collections::BTreeMap;
use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{elect_mise_cache_writers, mise_setup_step_p08};
use velnor_actions_workflow_renderer::steps::{TOOLS_CACHE_PATH, TOOLS_SAVE_USES};

use super::impl_renderer_fixtures::*;

/// `cache_save` carried by one job's Mise setup step, when present.
fn setup_save(job: &Job) -> Option<&str> {
    job.steps.iter().find_map(|step| match &step.kind {
        StepKind::Action { uses, with, .. } if uses.starts_with("jdx/mise-action@") => {
            with.get("cache_save").map(String::as_str)
        }
        _ => None,
    })
}

/// `Save Mise tools` steps carried by one job, in step order.
fn tools_saves(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect()
}

/// One Mise-setup job over an explicit cache key.
fn keyed_job(key: &str) -> Result<Job, RenderError> {
    Ok(Job {
        display_name: "Keyed".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![mise_setup_step_p08(&mise(), key)?],
    })
}

#[test]
fn mise_cache_writer_election_prefers_plan_then_lowest_id() -> Result<(), RenderError> {
    let shared = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let unique = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-bbbbbbbbbbbbbbbb";
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job(shared)?),
        ("rust-b".to_owned(), keyed_job(shared)?),
        ("rust-c".to_owned(), keyed_job(unique)?),
    ]);
    elect_mise_cache_writers(&mut jobs)?;
    assert_eq!(saved_key(&jobs["plan"]), Some(shared), "plan wins shared");
    assert!(tools_saves(&jobs["rust-b"]).is_empty(), "sharer saves none");
    assert_eq!(
        saved_key(&jobs["rust-c"]),
        Some(unique),
        "sole owner keeps its writer"
    );

    let mut jobs = BTreeMap::from([
        ("rust-b".to_owned(), keyed_job(shared)?),
        ("rust-a".to_owned(), keyed_job(shared)?),
    ]);
    elect_mise_cache_writers(&mut jobs)?;
    assert_eq!(
        saved_key(&jobs["rust-a"]),
        Some(shared),
        "lowest id wins without plan"
    );
    assert!(tools_saves(&jobs["rust-b"]).is_empty());
    Ok(())
}

/// The tools key one job's single save step archives, when exactly one.
fn saved_key(job: &Job) -> Option<&str> {
    let saves = tools_saves(job);
    if saves.len() != 1 {
        return None;
    }
    match &saves[0].kind {
        StepKind::Action { with, .. } => with.get("key").map(String::as_str),
        _ => None,
    }
}

#[test]
fn mise_cache_writer_election_saves_restore_only_and_push_gated() -> Result<(), RenderError> {
    let shared = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job(shared)?),
        ("rust-b".to_owned(), keyed_job(shared)?),
    ]);
    elect_mise_cache_writers(&mut jobs)?;
    for (id, job) in &jobs {
        assert_eq!(
            setup_save(job),
            Some("false"),
            "{id}: setups stay restore-only"
        );
    }
    let saves = tools_saves(&jobs["plan"]);
    assert_eq!(saves.len(), 1, "winner saves once");
    let save = saves[0];
    assert_eq!(save.condition.as_deref(), Some(CACHE_SAVE_CONDITION));
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOOLS_SAVE_USES);
    assert_eq!(with.get("key").map(String::as_str), Some(shared));
    assert_eq!(with.get("path").map(String::as_str), Some(TOOLS_CACHE_PATH));
    Ok(())
}

#[test]
fn mise_cache_writer_election_skips_keyless_and_reruns() -> Result<(), RenderError> {
    let shared = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let bare = Job {
        display_name: "Bare".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    };
    let mut jobs = BTreeMap::from([
        ("bare".to_owned(), bare),
        ("rust-a".to_owned(), keyed_job(shared)?),
    ]);
    elect_mise_cache_writers(&mut jobs)?;
    elect_mise_cache_writers(&mut jobs)?;
    assert!(jobs["bare"].steps.is_empty(), "keyless job untouched");
    assert_eq!(
        tools_saves(&jobs["rust-a"]).len(),
        1,
        "reruns add no second save"
    );
    Ok(())
}

#[test]
fn mise_cache_writer_election_leaves_foreign_setups_untouched() -> Result<(), RenderError> {
    let shared = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let mut foreign = keyed_job(shared)?;
    for step in &mut foreign.steps {
        if let StepKind::Action { with, .. } = &mut step.kind {
            with.insert("cache_save".to_owned(), "custom".to_owned());
        }
    }
    let mut jobs = BTreeMap::from([
        ("rust-a".to_owned(), keyed_job(shared)?),
        ("rust-z".to_owned(), foreign),
    ]);
    elect_mise_cache_writers(&mut jobs)?;
    assert_eq!(
        setup_save(&jobs["rust-z"]),
        Some("custom"),
        "foreign shapes stay untouched"
    );
    assert_eq!(
        saved_key(&jobs["rust-a"]),
        Some(shared),
        "lowest id wins the shared key"
    );
    Ok(())
}
