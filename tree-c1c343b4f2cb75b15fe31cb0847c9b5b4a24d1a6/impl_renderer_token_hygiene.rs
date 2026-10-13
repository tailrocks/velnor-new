//! Token hygiene: credential scoping and scrub gates.
use std::collections::BTreeMap;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, merge_step, plan_step, render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::*;

fn token_plan_job(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(name, argv, env)?,
            plan_step(),
        ],
    ))
}

fn render_fails_with(jobs: Vec<(String, velnor_actions_contract::Job)>, want: &str) {
    assert!(
        render_workflow_ir(
            &fixture_ir(jobs),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .is_err_and(|err| format!("{err:?}").contains(want)),
        "must fail with {want}"
    );
}

#[test]
fn token_hygiene_scopes_gh_token_to_plan() -> Result<(), RenderError> {
    let scoped = token_plan_job(
        "Plan",
        vec!["true".to_owned()],
        BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![scoped]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let bad_env = token_plan_job(
        "Leak",
        vec!["true".to_owned()],
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]),
    )?;
    render_fails_with(vec![bad_env], "credential_env");
    Ok(())
}

#[test]
fn token_hygiene_allows_final_fetch_token() -> Result<(), RenderError> {
    let (id, mut final_job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(
                "Prepare pinned tools",
                vec!["true".to_owned()],
                BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
            )?,
            merge_step(),
        ],
    );
    final_job.condition = Some("always()".to_owned());
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, (id, final_job)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn token_hygiene_rejects_prints_and_task_tokens() -> Result<(), RenderError> {
    let printed = token_plan_job(
        "Task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo $GH_TOKEN".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![printed], "token_in_run");
    let task_token = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![shell_step(
            "Run task",
            vec!["true".to_owned()],
            BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
        )?],
    );
    render_fails_with(vec![minimal_plan_job()?, task_token], "token_misplaced");
    Ok(())
}

#[test]
fn token_hygiene_allows_empty_scrub_and_rejects_all_seven_keys() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    let scrub: BTreeMap<String, String> = STEP_CREDENTIAL_DENYLIST
        .iter()
        .map(|key| ((*key).to_owned(), String::new()))
        .collect();
    let scrubbed = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![shell_step("Run task", vec!["true".to_owned()], scrub)?],
    );
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, scrubbed]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for key in STEP_CREDENTIAL_DENYLIST {
        let leaked = job(
            "velnor-task",
            "Task",
            vec!["plan".to_owned()],
            vec![shell_step(
                "Run task",
                vec!["true".to_owned()],
                BTreeMap::from([(key.to_owned(), "x".to_owned())]),
            )?],
        );
        let want = if key == "GH_TOKEN" {
            "token_misplaced"
        } else {
            "credential_env"
        };
        render_fails_with(vec![minimal_plan_job()?, leaked], want);
    }
    Ok(())
}
