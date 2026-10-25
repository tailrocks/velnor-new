//! P08 writer-election cases for tools and Tofu-provider cache keys.

use std::collections::BTreeMap;
use velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_SAVE_CONDITION as CACHE_SAVE_CONDITION;
use velnor_actions_contract::{
    Job, JobTimeout, PullRequestCachePolicy, Step, StepId, StepKind, StepRole,
};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::cache_p08::{
    ToolsCacheInputs, ToolsCachePayload, elect_cache_writers, elect_cache_writers_with_policy,
};
use velnor_actions_workflow_renderer::steps::{TOOLS_CACHE_PATHS, TOOLS_SAVE_USES};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDERS_SAVE_USES, tofu_providers_save_step,
};

use super::impl_renderer_fixtures::*;

/// `Save Mise tools` steps carried by one job, in step order.
fn tools_saves(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect()
}

/// One job with the canonical runtime identity and V2 restore for one tool set.
pub(crate) fn keyed_job(tool: &str) -> Result<Job, RenderError> {
    let specs = [tool.to_owned()];
    let payload = ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: LABEL,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &mise(),
        tool_specs: &specs,
        rustup_toolchain: None,
        rustup_components: &[],
        scale_set_image_source_sha: None,
    })?;
    let mut prelude = payload.runtime_prelude_step()?;
    prelude.condition = Some(
        velnor_actions_workflow_renderer::cache_p08::TOOLS_CACHE_PLAN_RUN_CONDITION.to_owned(),
    );
    let mut restore = payload.restore_step()?;
    restore.condition = Some(
        velnor_actions_workflow_renderer::cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION.to_owned(),
    );
    Ok(Job {
        services: Vec::new(),
        display_name: "Keyed".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![prelude, restore],
        cache_mode: None,
    })
}

#[test]
fn tools_cache_writer_election_prefers_plan_then_lowest_id() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job("jactionlint@2.3.0")?),
        ("rust-b".to_owned(), keyed_job("jactionlint@2.3.0")?),
        ("rust-c".to_owned(), keyed_job("shellcheck@0.11.0")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    assert_eq!(
        saved_key(&jobs["plan"]),
        restore_key(&jobs["plan"]),
        "plan wins shared"
    );
    assert_eq!(tools_saves(&jobs["rust-b"]).len(), 0, "sharer saves none");
    assert_eq!(
        saved_key(&jobs["rust-c"]),
        restore_key(&jobs["rust-c"]),
        "sole owner keeps its writer"
    );

    let mut jobs = BTreeMap::from([
        ("rust-b".to_owned(), keyed_job("jactionlint@2.3.0")?),
        ("rust-a".to_owned(), keyed_job("jactionlint@2.3.0")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    assert_eq!(
        saved_key(&jobs["rust-a"]),
        restore_key(&jobs["rust-a"]),
        "lowest id wins without plan"
    );
    assert_eq!(
        tools_saves(&jobs["rust-b"]),
        [] as [&velnor_actions_contract::Step; 0]
    );
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

#[test]
fn tools_cache_writer_saves_exact_payload_and_is_push_gated() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), keyed_job("jactionlint@2.3.0")?),
        ("rust-b".to_owned(), keyed_job("jactionlint@2.3.0")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    let saves = tools_saves(&jobs["plan"]);
    assert_eq!(saves.len(), 1, "winner saves once");
    let save = saves[0];
    assert_eq!(
        save.condition.as_deref(),
        Some(
            "success() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'"
        )
    );
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert_eq!(uses, TOOLS_SAVE_USES);
    assert_eq!(
        with.get("key").map(String::as_str),
        restore_key(&jobs["plan"])
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(TOOLS_CACHE_PATHS.join("\n").as_str())
    );
    let mut pr_jobs = BTreeMap::from([("plan".to_owned(), keyed_job("jactionlint@2.3.0")?)]);
    elect_cache_writers_with_policy(&mut pr_jobs, PullRequestCachePolicy::SameRepositoryScoped)?;
    assert_eq!(
        tools_saves(&pr_jobs["plan"])[0].condition.as_deref(),
        Some(
            "success() && ((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'"
        )
    );
    Ok(())
}

fn bare_job(display_name: &str) -> Job {
    Job {
        services: Vec::new(),
        display_name: display_name.to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
        cache_mode: None,
    }
}

#[test]
fn tools_cache_writer_skips_keyless_and_reruns() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        ("bare".to_owned(), bare_job("Bare")),
        ("rust-a".to_owned(), keyed_job("jactionlint@2.3.0")?),
    ]);
    elect_cache_writers(&mut jobs)?;
    elect_cache_writers(&mut jobs)?;
    assert_eq!(jobs["bare"].steps.len(), 0, "keyless job untouched");
    assert_eq!(
        tools_saves(&jobs["rust-a"]).len(),
        1,
        "reruns add no second save"
    );
    Ok(())
}

#[path = "impl_renderer_p08_elect/provider_tests.rs"]
mod provider_tests;
