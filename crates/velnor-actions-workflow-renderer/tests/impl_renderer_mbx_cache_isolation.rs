//! Stock MBX object-cache generation and hosted lane isolation.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::steps::shell_step;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn scale_set_token() -> Result<String, RenderError> {
    ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map(|selector| selector.token())
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))
}

fn render_mbx_job(id: &str, runs_on: &str) -> Result<String, RenderError> {
    let mbx_steps = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let mut built = job(
        id,
        "MBX job",
        Vec::new(),
        mbx_steps.into_iter().collect(),
    );
    built.1.runs_on = runs_on.to_owned();
    render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
}

fn assert_no_v17_only_inputs(text: &str) {
    assert!(!text.contains("isolate-objects-cache"), "{text}");
    assert!(!text.contains("cache-key-suffix"), "{text}");
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
}

#[test]
fn hosted_action_scopes_generation_to_its_job_and_keeps_stock_lifecycle() -> Result<(), RenderError>
{
    let text = render_mbx_job("demo", "ubuntu-26.04")?;
    let generation = format!("{}-job-demo", mbx_cache_generation("1.21.1"));
    assert!(text.contains("name: Restore MBX objects"), "{text}");
    assert!(text.contains("id: mbx"), "generic step id stays serialized");
    assert!(
        text.contains(&format!("cache-generation: {generation}")),
        "both generated cache-key families derive from this job-scoped generation:\n{text}"
    );
    assert!(text.contains("github-cache-mode: objects"), "{text}");
    assert!(text.contains("ACTIONS_CACHE_MODE: read"), "{text}");
    assert!(
        !text.contains("cache-key:"),
        "stock key construction stays active"
    );
    assert!(
        !text.contains("restore-keys:"),
        "stock fallback construction stays active"
    );
    assert_no_v17_only_inputs(&text);
    assert!(!text.contains("save-on-"), "{text}");
    assert!(!text.contains("MBX single bundle"), "{text}");
    assert!(!text.contains("mbx-bundle"), "{text}");
    Ok(())
}

#[test]
fn hosted_platforms_get_distinct_generations_and_scale_set_is_uncached() -> Result<(), RenderError>
{
    let linux = render_mbx_job("linux-mbx", "ubuntu-26.04")?;
    let macos = render_mbx_job("macos-mbx", "macos-15")?;
    for (text, id) in [(&linux, "linux-mbx"), (&macos, "macos-mbx")] {
        let generation = format!("{}-job-{id}", mbx_cache_generation("1.21.1"));
        assert!(
            text.contains(&format!("cache-generation: {generation}")),
            "{text}"
        );
        assert_no_v17_only_inputs(text);
    }
    assert_ne!(
        mbx_cache_generation("1.21.1") + "-job-linux-mbx",
        mbx_cache_generation("1.21.1") + "-job-macos-mbx"
    );

    let scale_set = scale_set_token()?;
    let local = render_mbx_job("local-mbx", &scale_set)?;
    assert!(local.contains("backend: local"), "{local}");
    assert!(local.contains("github-cache-mode: objects"), "{local}");
    assert!(!local.contains("cache-generation:"), "{local}");
    assert_no_v17_only_inputs(&local);
    Ok(())
}

#[test]
fn paired_lanes_keep_one_action_per_job_outside_shared_composites() -> Result<(), RenderError> {
    let uses = mbx_uses();
    let before = shell_step("Before MBX", vec!["true".to_owned()], Default::default())?;
    let build = shell_step("Build", vec!["true".to_owned()], Default::default())?;
    let test = shell_step("Test", vec!["true".to_owned()], Default::default())?;
    let mut steps = vec![before];
    steps.extend(mbx_tool_steps(&uses, "1.21.1", "1.98.1")?);
    steps.extend([build, test]);
    let hosted = job("demo__hosted", "Demo hosted", Vec::new(), steps.clone());
    let mut local = job("demo__local", "Demo local", Vec::new(), steps);
    local.1.runs_on = scale_set_token()?;
    let (plan_id, mut plan) = minimal_plan_job()?;
    plan.steps.insert(1, acquire_fixture()?);
    let mut jobs = vec![(plan_id, plan)];
    jobs.push(hosted);
    jobs.push(local);
    let rendered = render_workflow_ir_strict_shared(
        &fixture_ir(jobs),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise_set(),
    )?;

    for id in ["demo__hosted", "demo__local"] {
        let names = step_names(&rendered.yaml, id);
        assert_eq!(
            names
                .iter()
                .filter(|name| *name == "Restore MBX objects")
                .count(),
            1,
            "one action owns each job's complete restore/post-save lifecycle: {names:?}"
        );
    }
    let hosted_generation = format!("{}-job-demo__hosted", mbx_cache_generation("1.21.1"));
    assert!(
        rendered
            .yaml
            .contains(&format!("cache-generation: {hosted_generation}")),
        "{0}",
        rendered.yaml
    );
    assert!(
        rendered.yaml.contains("backend: local"),
        "{}",
        rendered.yaml
    );
    assert_eq!(
        rendered.shared.len(),
        2,
        "two ordered common runs are factored"
    );
    for file in &rendered.shared {
        assert!(
            !file.bytes.contains("Restore MBX objects"),
            "{}",
            file.bytes
        );
        assert!(
            !file.bytes.contains("jdx/mr-boxington-action"),
            "{}",
            file.bytes
        );
    }
    Ok(())
}

#[test]
fn duplicate_mbx_actions_fail_closed() -> Result<(), RenderError> {
    let uses = mbx_uses();
    let mut steps: Vec<_> = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?
        .into_iter()
        .collect();
    steps.extend(mbx_tool_steps(&uses, "1.21.1", "1.98.1")?);
    let result = render_workflow_ir(
        &fixture_ir(vec![job(
            "duplicate",
            "Duplicate",
            Vec::new(),
            steps,
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(matches!(
        result,
        Err(RenderError::InvalidWorkflow(message)) if message == "multiple_mbx_actions:duplicate"
    ));
    Ok(())
}
