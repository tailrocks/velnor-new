//! PR cache policy reaches generated MBX keys, conditions, and shared lanes.

use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::{PullRequestCachePolicy, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;
use super::impl_renderer_mbx_bundle::{mbx_job, mbx_job_with_checkout};

#[test]
fn opt_in_reaches_key_prep_and_elected_save_conditions() -> Result<(), RenderError> {
    let mut context = fixture_ctx();
    context.pull_request_cache_policy = PullRequestCachePolicy::SameRepositoryScoped;
    let text = render_workflow_ir(
        &fixture_ir(vec![mbx_job("demo", "1.21.1")?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )?;

    let key = step_window(&text, "Prepare MBX bundle key", "Restore MBX single bundle");
    for required in [
        "MBX_PR_CACHE_POLICY: same-repository-scoped",
        "github.event_name",
        "github.repository",
        "github.event.pull_request.head.repo.full_name",
        "github.event.pull_request.base.repo.full_name",
        "toJSON(github.event.pull_request.head.repo.fork)",
        "github.event.pull_request.number",
        "github.event.pull_request.head.sha",
        "same-repository-pr-${MBX_PR_NUMBER}-${MBX_PR_HEAD_SHA}",
    ] {
        assert!(key.contains(required), "{required}: {key}");
    }
    assert!(
        key.contains("MBX_HEAD_REPOSITORY_FORK") && key.contains("false"),
        "{key}"
    );

    let export = step_window(&text, "Export MBX single bundle", "Save MBX single bundle");
    let save = text
        .get(
            text.find("name: Save MBX single bundle").ok_or_else(|| {
                RenderError::InvalidWorkflow("missing_mbx_save_step".to_owned())
            })?..,
        )
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_save_window".to_owned()))?;
    for condition in [export, save] {
        assert!(
            condition.contains("github.event_name == 'push'"),
            "{condition}"
        );
        assert!(
            condition.contains("github.ref_name == github.event.repository.default_branch"),
            "{condition}"
        );
        assert!(
            condition.contains("github.event_name == 'pull_request'"),
            "{condition}"
        );
        assert!(
            condition.contains("steps.mbx-bundle-key.outputs.pr-cache-allowed == 'true'"),
            "{condition}"
        );
    }
    assert_eq!(text.matches("name: Save MBX single bundle").count(), 1);
    Ok(())
}

#[test]
fn read_only_render_keeps_existing_push_only_writer_gate() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![mbx_job("demo", "1.21.1")?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let key = step_window(&text, "Prepare MBX bundle key", "Restore MBX single bundle");
    assert!(!key.contains("MBX_PR_CACHE_POLICY"), "{key}");
    let save = text
        .get(
            text.find("name: Save MBX single bundle").ok_or_else(|| {
                RenderError::InvalidWorkflow("missing_mbx_save_step".to_owned())
            })?..,
        )
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_save_window".to_owned()))?;
    assert!(save.contains("github.event_name == 'push'"), "{save}");
    assert!(
        save.contains("github.ref_name == github.event.repository.default_branch"),
        "{save}"
    );
    assert!(!save.contains("pull_request"), "{save}");
    assert!(!save.contains("pr-cache-allowed"), "{save}");
    let restore = step_window(
        &text,
        "Restore MBX single bundle",
        "Import MBX single bundle",
    );
    assert!(restore.contains("restore-keys:"), "{restore}");
    Ok(())
}

#[test]
fn qualification_roles_restore_exact_keys() -> Result<(), RenderError> {
    let jobs = vec![
        qualification_job("mbx-probe-seed", true)?,
        qualification_job("mbx-probe-read", false)?,
        qualification_job("mbx-probe-corrupt-read", false)?,
    ];
    let mut context = fixture_ctx();
    context.pull_request_cache_policy = PullRequestCachePolicy::SameRepositoryScoped;
    let text = render_workflow_ir(
        &fixture_ir(jobs),
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )?;
    assert_eq!(text.matches("name: Restore MBX single bundle").count(), 3);
    assert_eq!(text.matches("restore-keys:").count(), 0, "{text}");
    assert!(!text.contains("MBX_PR_CACHE_POLICY"), "{text}");
    let export = step_window(&text, "Export MBX single bundle", "Save MBX single bundle");
    let save = text
        .get(
            text.find("name: Save MBX single bundle").ok_or_else(|| {
                RenderError::InvalidWorkflow("missing_mbx_save_step".to_owned())
            })?..,
        )
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_save_window".to_owned()))?;
    for condition in [export, save] {
        assert!(!condition.contains("pull_request"), "{condition}");
        assert!(!condition.contains("pr-cache-allowed"), "{condition}");
    }
    for evidence in [
        "export-start",
        "export-end",
        "gc-start",
        "gc-end",
        "snapshot export-complete",
        "snapshot gc-complete",
        "sampler.sh",
        "MBX_QUALIFICATION_SAMPLE_INTERVAL",
        "MBX_QUALIFICATION_IMPORT_RECEIPT",
        "exit_status=%s",
        "id: mbx-bundle-save",
    ] {
        assert!(text.contains(evidence), "{evidence}: {text}");
    }
    Ok(())
}

#[test]
fn paired_qualification_lanes_keep_read_only_policy_under_opt_in() -> Result<(), RenderError> {
    let seed = qualification_job("mbx-pr-qualification-seed", true)?;
    let hosted = qualification_job_with_checkout("mbx-pr-qualification__hosted", false)?;
    let mut local = qualification_job_with_checkout("mbx-pr-qualification__local", false)?;
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    local.1.runs_on = selector.token();
    let mut context = fixture_ctx();
    context.pull_request_cache_policy = PullRequestCachePolicy::SameRepositoryScoped;
    let rendered = render_workflow_ir_strict_shared(
        &fixture_ir(vec![seed, hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &mise(),
    )?;

    assert_eq!(
        rendered
            .yaml
            .matches("name: Export MBX single bundle")
            .count(),
        1
    );
    assert_eq!(
        rendered
            .yaml
            .matches("name: Save MBX single bundle")
            .count(),
        1
    );
    assert!(
        !rendered.yaml.contains("pr-cache-allowed"),
        "{}",
        rendered.yaml
    );
    assert!(!rendered.yaml.contains("MBX_PR_CACHE_POLICY"));
    assert_eq!(rendered.yaml.matches("restore-keys:").count(), 0);
    let export = step_window(
        &rendered.yaml,
        "Export MBX single bundle",
        "Save MBX single bundle",
    );
    let save = step_window(&rendered.yaml, "Save MBX single bundle", "Save Mise tools");
    for condition in [export, save] {
        assert!(
            condition.contains("github.event_name == 'push'"),
            "{condition}"
        );
        assert!(!condition.contains("pull_request"), "{condition}");
        assert!(!condition.contains("pr-cache-allowed"), "{condition}");
    }
    let composite = rendered
        .shared
        .first()
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_shared_action".to_owned()))?;
    assert!(!composite.bytes.contains("mbx-pr-cache-allowed:"));
    assert!(!composite.bytes.contains("pr-cache-allowed"));
    Ok(())
}

#[test]
fn paired_lanes_export_pr_authorization_and_keep_one_writer() -> Result<(), RenderError> {
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
    assert_eq!(
        rendered
            .yaml
            .matches("name: Export MBX single bundle")
            .count(),
        1
    );
    assert_eq!(
        rendered
            .yaml
            .matches("name: Save MBX single bundle")
            .count(),
        1
    );
    let save_start = rendered
        .yaml
        .find("name: Save MBX single bundle")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_mbx_save_step".to_owned()))?;
    let save = &rendered.yaml[save_start..];
    assert!(
        save.contains("steps.mbx-lane-cache.outputs.mbx-pr-cache-allowed == 'true'"),
        "{save}"
    );
    assert!(
        !save.contains("steps.mbx-bundle-key.outputs.pr-cache-allowed"),
        "{save}"
    );
    let composite = rendered
        .shared
        .first()
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_shared_action".to_owned()))?;
    assert!(
        composite.bytes.contains("mbx-pr-cache-allowed:"),
        "{}",
        composite.bytes
    );
    assert!(
        composite
            .bytes
            .contains("steps.mbx-bundle-key.outputs.pr-cache-allowed"),
        "{}",
        composite.bytes
    );
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

fn qualification_job(
    id: &str,
    writer: bool,
) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    let (id, mut job) = mbx_job(id, "1.21.1")?;
    add_qualification_scope(&mut job, writer)?;
    Ok((id, job))
}

fn qualification_job_with_checkout(
    id: &str,
    writer: bool,
) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    let (id, mut job) = mbx_job_with_checkout(id, "1.21.1")?;
    add_qualification_scope(&mut job, writer)?;
    Ok((id, job))
}

fn add_qualification_scope(
    job: &mut velnor_actions_contract::Job,
    writer: bool,
) -> Result<(), RenderError> {
    let Some(step) = job.steps.iter_mut().find(|step| match &step.kind {
        StepKind::Action { uses, .. } => uses.contains("mr-boxington-action@"),
        StepKind::Shell { .. } | StepKind::Internal { .. } => false,
    }) else {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_action".to_owned(),
        ));
    };
    let StepKind::Action { with, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow("bad_mbx_action".to_owned()));
    };
    with.insert(
        "velnor-cache-scope".to_owned(),
        "qualification-mbx-v1/pr-exact-only".to_owned(),
    );
    with.insert(
        "velnor-cache-writer".to_owned(),
        if writer { "true" } else { "false" }.to_owned(),
    );
    Ok(())
}
