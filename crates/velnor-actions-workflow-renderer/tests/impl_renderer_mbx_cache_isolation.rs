//! Stock MBX object-cache lifecycle and hosted lane isolation.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::steps::{mbx_objects_step, shell_step};
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

fn assert_shared_runs_bracket_mbx(names: &[String]) {
    let shared: Vec<_> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| (name == "Run shared steps").then_some(index))
        .collect();
    let mbx: Vec<_> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| (name == "Restore MBX objects").then_some(index))
        .collect();
    assert_eq!(shared.len(), 2, "{names:?}");
    assert_eq!(mbx.len(), 1, "{names:?}");
    assert!(shared[0] < mbx[0] && mbx[0] < shared[1], "{names:?}");
}

fn render_mbx_job(id: &str, runs_on: &str) -> Result<String, RenderError> {
    let mut built = job(
        id,
        "MBX job",
        Vec::new(),
        vec![mbx_objects_step(&mbx_uses(), false, "1.21.1")?],
    );
    built.1.runs_on = runs_on.to_owned();
    render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
}

#[test]
fn stock_action_owns_restore_and_post_save() -> Result<(), RenderError> {
    let text = render_mbx_job("demo", "ubuntu-26.04")?;
    assert!(text.contains("name: Restore MBX objects"), "{text}");
    assert!(text.contains("id: mbx"), "generic step id stays serialized");
    assert!(text.contains("isolate-objects-cache: \"true\""), "{text}");
    assert!(
        text.contains("cache-key-suffix: ${{ github.job }}"),
        "{text}"
    );
    assert!(text.contains("version: 1.21.1"), "{text}");
    assert!(text.contains("cache-generation:"), "{text}");
    assert!(text.contains("MBX_GC_AUTO: \"1\""), "{text}");
    assert!(!text.contains("ACTIONS_CACHE_MODE"), "{text}");
    assert!(!text.contains("save-on-"), "{text}");
    assert!(!text.contains("MBX single bundle"), "{text}");
    assert!(!text.contains("mbx-bundle"), "{text}");
    Ok(())
}

#[test]
fn macos_is_hosted_while_scale_set_keeps_its_shared_cache() -> Result<(), RenderError> {
    let macos = render_mbx_job("macos-mbx", "macos-15")?;
    assert!(macos.contains("isolate-objects-cache: \"true\""), "{macos}");
    assert!(
        macos.contains("cache-key-suffix: ${{ github.job }}"),
        "{macos}"
    );
    assert!(macos.contains("MBX_GC_AUTO: \"1\""), "{macos}");

    let scale_set = scale_set_token()?;
    let local = render_mbx_job("local-mbx", &scale_set)?;
    assert!(!local.contains("isolate-objects-cache"), "{local}");
    assert!(!local.contains("cache-key-suffix"), "{local}");
    assert!(!local.contains("MBX_GC_AUTO"), "{local}");
    assert!(!local.contains("ACTIONS_CACHE_MODE"), "{local}");
    Ok(())
}

#[test]
fn paired_lanes_keep_ordered_shared_runs_around_job_local_action() -> Result<(), RenderError> {
    let uses = mbx_uses();
    let before = shell_step("Before MBX", vec!["true".to_owned()], Default::default())?;
    let build = shell_step("Build", vec!["true".to_owned()], Default::default())?;
    let test = shell_step("Test", vec!["true".to_owned()], Default::default())?;
    let steps = vec![
        before,
        mbx_objects_step(&uses, false, "1.21.1")?,
        build,
        test,
    ];
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

    assert_shared_runs_bracket_mbx(&step_names(&rendered.yaml, "demo__hosted"));
    assert_shared_runs_bracket_mbx(&step_names(&rendered.yaml, "demo__local"));
    assert_eq!(
        rendered
            .yaml
            .matches("isolate-objects-cache: \"true\"")
            .count(),
        1,
        "only hosted job gets the isolated cache key"
    );

    let first = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/shared-lanes/demo/segment-0/action.yml")
        .expect("first shared segment");
    let second = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/shared-lanes/demo/segment-1/action.yml")
        .expect("second shared segment");
    assert!(first.bytes.contains("Before MBX"), "{}", first.bytes);
    assert!(
        !first.bytes.contains("Restore MBX objects"),
        "{}",
        first.bytes
    );
    assert!(second.bytes.contains("Build"), "{}", second.bytes);
    assert!(second.bytes.contains("Test"), "{}", second.bytes);
    assert!(
        !second.bytes.contains("Restore MBX objects"),
        "{}",
        second.bytes
    );
    Ok(())
}
