//! Hosted MBX saves one bundle outside the store. The action post does not.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::steps::mbx_objects_step;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let mbx = mbx_objects_step(&mbx_uses(), false, "1.21.1")?;
    let mut built = job(id, "MBX job", Vec::new(), vec![mbx]);
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

#[test]
fn hosted_save_is_one_bundle_outside_the_store() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let restore = text
        .find("name: Restore MBX objects")
        .expect("restore step");
    let export = text
        .find("name: Export MBX single bundle")
        .expect("export step");
    let save = text
        .find("name: Save MBX single bundle")
        .expect("save step");
    assert!(restore < export && export < save, "{text}");
    let action = &text[restore..export];
    assert!(action.contains("id: mbx"), "{action}");
    assert!(action.contains("ACTIONS_CACHE_MODE: read"), "{action}");
    assert!(!action.contains("write"), "{action}");
    let script = &text[export..save];
    let gc = script.find("mbx gc").expect("reclaim");
    let exported = script.find("cache export").expect("export");
    let durable = script.find("test -d").expect("bundle check");
    let deleted = script
        .find(r#"rm -rf \"$store\""#)
        .unwrap_or_else(|| panic!("store delete missing:\n{script}"));
    assert!(
        gc < exported && exported < durable && durable < deleted,
        "{script}"
    );
    assert!(
        script.contains("$RUNNER_TEMP/mbx-single-bundle"),
        "{script}"
    );
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");
    assert!(!script.contains("github-actions-cache-v1"), "{script}");
    let saved = &text[save..];
    assert!(
        saved.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{saved}"
    );
    assert!(
        saved.contains("key: ${{ steps.mbx.outputs.cache-primary-key }}"),
        "{saved}"
    );
    assert!(
        saved.contains("steps.mbx-export.outputs.ready == 'true'"),
        "{saved}"
    );
    assert!(saved.contains("github.event_name == 'push'"), "{saved}");
    assert!(!saved.contains("pull_request"), "{saved}");
    assert!(!text.contains("continue-on-error"), "{text}");
    Ok(())
}

#[test]
fn scale_set_save_matches_and_skips_hosted_gc_env() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    assert!(text.contains("name: Export MBX single bundle"), "{text}");
    assert!(text.contains("ACTIONS_CACHE_MODE: read"), "{text}");
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    Ok(())
}
