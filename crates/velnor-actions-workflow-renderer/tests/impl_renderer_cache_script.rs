//! Shared renderer-owned runtime identity script emission and cold-lane routing.

use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::{WorkflowIr, WorkflowPolicy};
use velnor_actions_workflow_renderer::render::RenderedWorkflow;
use velnor_actions_workflow_renderer::{RenderError, checkout_step};

use super::impl_renderer_fixtures::*;

const IDENTITY_SCRIPT_PATH: &str = ".github/scripts/velnor-tools-cache-identity.sh";
const IDENTITY_SCRIPT_NAME: &str = "velnor-tools-cache-identity.sh";
const RESTORE_ACTION_PATH: &str = ".github/actions/velnor-tools-cache-restore/action.yml";
const UBUNTU26_ACTION: &str = "./.github/actions/velnor-tools-prelude-u26";
const IDENTITY_STEP: &str = "V2 identity";
const RESTORE_STEP: &str = "Restore Mise tools";

fn render_cache_fixture() -> Result<RenderedWorkflow, RenderError> {
    let tool_step = || {
        scrubbed_shell_step(
            "Run actionlint",
            mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
        )
    };
    let hosted = job(
        "rust-0__hosted",
        "Rust hosted",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, tool_step()?],
    );
    let mut local = job(
        "rust-0__local",
        "Rust local",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, tool_step()?],
    );
    local.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?
    .token();
    let unpaired = job(
        "unpaired",
        "Unpaired",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, tool_step()?],
    );
    let ir = fixture_ir(vec![hosted, local, unpaired]);
    velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
}

fn assert_job_order(yaml: &str, id: &str) {
    let names = step_names(yaml, id);
    let checkout = names.iter().position(|name| name == "Checkout");
    let identity = names.iter().position(|name| name == IDENTITY_STEP);
    let restore = names.iter().position(|name| name == RESTORE_STEP);
    assert!(
        matches!((checkout, identity, restore), (Some(a), Some(b), Some(c)) if a < b && b < c),
        "cache prelude order for {id}: {names:?}"
    );
}

fn assert_one_marked_script(rendered: &RenderedWorkflow) -> Result<(), RenderError> {
    let scripts: Vec<_> = rendered
        .shared
        .iter()
        .filter(|file| file.path == IDENTITY_SCRIPT_PATH)
        .collect();
    assert_eq!(scripts.len(), 1);
    let marker = velnor_actions_workflow_renderer::marker::marker_for_version(VERSION)?;
    assert!(scripts[0].bytes.starts_with(&format!("{marker}\n")));
    assert!(scripts[0].bytes.contains("runtime-identity.$$"));
    assert!(!rendered.yaml.contains("runtime-identity.$$"));
    let actions: Vec<_> = rendered
        .shared
        .iter()
        .filter(|file| {
            file.path.starts_with(".github/actions/u") && file.path.ends_with("/action.yml")
        })
        .collect();
    assert_eq!(actions.len(), 1);
    for action in actions {
        assert!(action.bytes.starts_with(&format!("{marker}\n")));
        assert_eq!(action.bytes.matches(IDENTITY_SCRIPT_NAME).count(), 1);
        assert!(
            action.bytes.contains("VELNOR_CACHE_LANE: ubuntu-26.04"),
            "{} must bind its exact lane",
            action.path
        );
    }
    let preludes: Vec<_> = rendered
        .shared
        .iter()
        .filter(|file| file.path == ".github/actions/velnor-tools-prelude-u26/action.yml")
        .collect();
    assert_eq!(preludes.len(), 1);
    assert!(preludes[0].bytes.contains("id: v2"));
    assert!(preludes[0].bytes.contains("steps.v2.outputs.enabled"));
    assert!(preludes[0].bytes.contains("steps.v2.outputs.identity"));
    assert!(preludes[0].bytes.contains("velnor-tool-seed"));
    assert_eq!(rendered.yaml.matches(UBUNTU26_ACTION).count(), 2);
    let restore_action = rendered
        .shared
        .iter()
        .find(|file| file.path == RESTORE_ACTION_PATH)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("missing_tools_restore_composite".to_owned())
        })?;
    assert_eq!(
        rendered
            .shared
            .iter()
            .filter(|file| file.path == RESTORE_ACTION_PATH)
            .count(),
        1,
        "the composite source is emitted once"
    );
    assert!(restore_action.bytes.contains("actions/cache/restore@"));
    assert!(restore_action.bytes.contains("key: ${{ inputs.key }}"));
    assert!(restore_action.bytes.starts_with(&format!("{marker}\n")));
    assert!(
        rendered
            .yaml
            .contains(velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES)
    );
    assert_eq!(
        rendered
            .yaml
            .matches(velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES)
            .count(),
        2,
        "paired and unpaired hosted callers reference one shared composite"
    );
    assert!(rendered.yaml.contains(
        "uses: ./.github/actions/velnor-tools-cache-restore # zizmor: ignore[self-repository]"
    ));
    assert!(!rendered.yaml.contains(IDENTITY_SCRIPT_PATH));
    Ok(())
}

#[test]
fn shared_runtime_identity_script_covers_hosted_and_unpaired_jobs_once() -> Result<(), RenderError>
{
    let rendered = render_cache_fixture()?;
    assert_one_marked_script(&rendered)?;
    for id in ["rust-0__hosted", "unpaired"] {
        assert_job_order(&rendered.yaml, id);
    }
    let local = step_names(&rendered.yaml, "rust-0__local");
    assert!(
        !local
            .iter()
            .any(|name| name == IDENTITY_STEP || name == RESTORE_STEP)
    );
    Ok(())
}

#[test]
fn cache_free_render_does_not_emit_runtime_identity_script() -> Result<(), RenderError> {
    let plain = job(
        "plain",
        "Plain",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step("No Mise tools", vec!["echo".to_owned(), "plain".to_owned()])?,
        ],
    );
    let ir: WorkflowIr = fixture_ir(vec![plain]);
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )?;
    assert!(rendered.shared.is_empty(), "{:?}", rendered.shared);
    assert!(!rendered.yaml.contains("velnor-tools-cache-identity"));
    assert!(
        !rendered
            .yaml
            .contains(velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES)
    );
    Ok(())
}
