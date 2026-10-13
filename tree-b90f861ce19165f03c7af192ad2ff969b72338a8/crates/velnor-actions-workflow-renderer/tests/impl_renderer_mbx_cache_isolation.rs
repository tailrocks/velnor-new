//! Hosted MBX actions isolate caches by job; native actions retain shared keys.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::steps::{
    crate_job_report_upload_step, matrix_report_upload_step, mbx_objects_step,
};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let restore = mbx_objects_step(&mbx_uses(), false, "1.22.0")?;
    let mut built = job(
        id,
        "MBX job",
        Vec::new(),
        vec![
            restore,
            matrix_report_upload_step()?,
            crate_job_report_upload_step(id)?,
        ],
    );
    if scale_set {
        let selector = ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
        built.1.runs_on = selector.token();
    }
    render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
}

fn mbx_action(text: &str) -> &str {
    let marker = "name: Restore MBX objects";
    assert!(text.contains(marker), "missing MBX restore step:\n{text}");
    let section = text
        .split_once(marker)
        .map_or(text, |(before, _)| &text[before.len()..]);
    let end = section.find("\n      - name:").unwrap_or(section.len());
    &section[..end]
}

fn assert_shared_action_identity(text: &str) {
    let action = mbx_action(text);
    assert!(action.contains("version: 1.22.0"), "{action}");
    assert!(
        action.contains(&format!(
            "cache-generation: {}",
            mbx_cache_generation("1.22.0")
        )),
        "{action}"
    );
    assert!(!action.contains("ACTIONS_CACHE_MODE"), "{action}");
    assert!(!action.contains("steps.mbx.outputs."), "{action}");
    assert!(!action.contains("id: mbx\n"), "{action}");
    assert!(!text.contains("Export MBX single bundle"), "{text}");
    assert!(!text.contains("Save MBX single bundle"), "{text}");
}

#[test]
fn hosted_ci_isolates_mbx_cache_by_github_job() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    assert_shared_action_identity(&text);
    let action = mbx_action(&text);
    assert!(
        action.contains("isolate-objects-cache: \"true\""),
        "{action}"
    );
    assert!(
        action.contains("cache-key-suffix: ${{ github.job }}"),
        "{action}"
    );
    assert!(text.contains("MBX_GC_AUTO: \"1\""), "{text}");
    assert!(
        text.contains("velnor-matrix-r${{ github.run_id }}-a${{ github.run_attempt }}-${{ matrix.matrix_key }}"),
        "matrix report identity changed:\n{text}"
    );
    assert!(
        text.contains("velnor-crate-r${{ github.run_id }}-a${{ github.run_attempt }}-demo"),
        "crate report identity changed:\n{text}"
    );
    Ok(())
}

#[test]
fn scale_set_keeps_shared_mbx_cache_generation() -> Result<(), RenderError> {
    let text = render_mbx("demo", true)?;
    assert_shared_action_identity(&text);
    let action = mbx_action(&text);
    assert!(!action.contains("isolate-objects-cache"), "{action}");
    assert!(!action.contains("cache-key-suffix"), "{action}");
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    Ok(())
}

#[test]
fn hosted_and_scale_set_mbx_pair_remains_inline() -> Result<(), RenderError> {
    let restore = mbx_objects_step(&mbx_uses(), false, "1.22.0")?;
    let hosted = job(
        "rust-0__hosted",
        "Hosted",
        Vec::new(),
        vec![restore.clone()],
    );
    let mut local = job("rust-0__local", "Scale set", Vec::new(), vec![restore]);
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    local.1.runs_on = selector.token();
    let text = render_workflow_ir(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;

    assert_eq!(
        text.matches("name: Restore MBX objects").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("isolate-objects-cache: \"true\"").count(),
        1,
        "{text}"
    );
    assert_eq!(
        text.matches("cache-key-suffix: ${{ github.job }}").count(),
        1,
        "{text}"
    );
    Ok(())
}
