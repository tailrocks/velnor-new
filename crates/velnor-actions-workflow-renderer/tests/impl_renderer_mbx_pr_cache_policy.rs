//! Typed PR cache policy reaches generated MBX keys and elected saves.

use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::{PullRequestCachePolicy, WorkflowPolicy};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

#[test]
fn same_repository_policy_reaches_keys_and_one_lane_writer() -> Result<(), RenderError> {
    let hosted = mbx_job_with_checkout("rust-demo__hosted", "1.21.1")?;
    let mut local = mbx_job_with_checkout("rust-demo__local", "1.21.1")?;
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    local.1.runs_on = selector.token();
    let mut context = fixture_ctx();
    context.pull_request_cache_policy = PullRequestCachePolicy::SameRepositoryScoped;
    let rendered = render_workflow_ir_strict_shared(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &mise(),
    )?;
    let text = rendered.yaml;

    assert_eq!(text.matches("name: Export MBX single bundle").count(), 1);
    assert_eq!(text.matches("name: Save MBX single bundle").count(), 1);
    let key = step_window(
        &text,
        "Prepare MBX cache identity",
        "Prepare MBX local cache store",
    );
    for required in [
        "MBX_PR_CACHE_POLICY: same-repository-scoped",
        "github.event_name",
        "github.repository",
        "github.event.pull_request.head.repo.full_name",
        "github.event.pull_request.base.repo.full_name",
        "toJSON(github.event.pull_request.head.repo.fork)",
        "github.event.pull_request.number",
        "github.event.pull_request.head.sha",
        "same-repository-pr-\\\\${MBX_PR_NUMBER}-\\\\${MBX_PR_HEAD_SHA}",
    ] {
        assert!(key.contains(required), "missing `{required}`: {key}");
    }

    let save = text
        .get(
            text.find("name: Save MBX single bundle").ok_or_else(|| {
                RenderError::InvalidWorkflow("missing_mbx_save_step".to_owned())
            })?..,
        )
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_save_window".to_owned()))?;
    assert!(
        save.contains("steps.mbx-cache-key.outputs.pr-cache-allowed == 'true'"),
        "{save}"
    );
    assert!(save.contains("github.event_name == 'push'"), "{save}");
    assert!(
        save.contains("github.event_name == 'pull_request'"),
        "{save}"
    );
    Ok(())
}

#[test]
fn default_read_only_policy_keeps_the_protected_push_gate() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![mbx_job("demo", "1.21.1")?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let start = text
        .find("name: Restore MBX objects")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_mbx_restore".to_owned()))?;
    let action = &text[start..];
    assert!(!action.contains("MBX_PR_CACHE_POLICY"), "{action}");
    assert!(
        action.contains("ACTIONS_CACHE_MODE: ${{ github.event_name == 'push'"),
        "{action}"
    );
    assert!(action.contains("github.ref_protected == true"), "{action}");
    assert!(!action.contains("pull_request"), "{action}");
    Ok(())
}

fn step_window<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let Some(start) = text.find(&format!("name: {start}")) else {
        return "";
    };
    let Some(relative_end) = text[start..].find(&format!("name: {end}")) else {
        return "";
    };
    &text[start..start + relative_end]
}
