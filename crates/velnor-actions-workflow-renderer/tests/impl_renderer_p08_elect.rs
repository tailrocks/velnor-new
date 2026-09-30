//! P08 writer-election cases: one Mise-cache saver per cache key.

use std::collections::BTreeMap;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION_EXPR;
use velnor_actions_contract::{Job, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{elect_mise_cache_writers, mise_setup_step_p08};

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

/// One Mise-setup job over an explicit cache key.
fn keyed_job(key: &str) -> Result<Job, RenderError> {
    Ok(Job {
        display_name: "Keyed".to_owned(),
        runs_on: LABEL.to_owned(),
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
    elect_mise_cache_writers(&mut jobs);
    assert_eq!(setup_save(&jobs["plan"]), Some(CACHE_SAVE_CONDITION_EXPR));
    assert_eq!(setup_save(&jobs["rust-b"]), Some("false"));
    assert_eq!(
        setup_save(&jobs["rust-c"]),
        Some(CACHE_SAVE_CONDITION_EXPR),
        "sole owner keeps its writer"
    );

    let mut jobs = BTreeMap::from([
        ("rust-b".to_owned(), keyed_job(shared)?),
        ("rust-a".to_owned(), keyed_job(shared)?),
    ]);
    elect_mise_cache_writers(&mut jobs);
    assert_eq!(
        setup_save(&jobs["rust-a"]),
        Some(CACHE_SAVE_CONDITION_EXPR),
        "lowest id wins without plan"
    );
    assert_eq!(setup_save(&jobs["rust-b"]), Some("false"));
    Ok(())
}

#[test]
fn mise_cache_writer_election_skips_keyless_and_foreign_shapes() -> Result<(), RenderError> {
    let shared = "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let bare = Job {
        display_name: "Bare".to_owned(),
        runs_on: LABEL.to_owned(),
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    };
    let mut foreign = keyed_job(shared)?;
    for step in &mut foreign.steps {
        if let StepKind::Action { with, .. } = &mut step.kind {
            with.insert("cache_save".to_owned(), "custom".to_owned());
        }
    }
    let mut jobs = BTreeMap::from([
        ("bare".to_owned(), bare),
        ("rust-a".to_owned(), keyed_job(shared)?),
        ("rust-z".to_owned(), foreign),
    ]);
    elect_mise_cache_writers(&mut jobs);
    assert_eq!(
        setup_save(&jobs["rust-z"]),
        Some("custom"),
        "foreign shapes stay untouched"
    );
    assert_eq!(
        setup_save(&jobs["rust-a"]),
        Some(CACHE_SAVE_CONDITION_EXPR),
        "lowest id wins the shared key"
    );
    Ok(())
}
