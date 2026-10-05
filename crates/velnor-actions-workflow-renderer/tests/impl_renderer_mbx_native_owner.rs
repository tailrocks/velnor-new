//! The pinned MBX action is the sole object-cache owner.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn action_ref() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx_job(scale_set: bool) -> Result<String, RenderError> {
    let steps = mbx_tool_steps(&action_ref(), TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN)?;
    let mut built = job("mbx-job", "MBX job", Vec::new(), steps.into());
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
fn native_action_replaces_the_manual_bundle_owner() -> Result<(), RenderError> {
    for scale_set in [false, true] {
        let yaml = render_mbx_job(scale_set)?;
        assert!(yaml.contains("uses: jdx/mr-boxington-action@"), "{yaml}");
        assert!(yaml.contains("version: 1.22.0"), "{yaml}");
        assert!(yaml.contains("github-cache-mode: objects"), "{yaml}");
        assert!(yaml.contains("MBX_GC_AUTO: \"1\""), "{yaml}");
        assert!(yaml.contains("MBX_SHARE_OUT_DIR: \"0\""), "{yaml}");
        assert!(
            yaml.contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' }}"),
            "{yaml}"
        );
        for removed in [
            "Export MBX single bundle",
            "Save MBX single bundle",
            "Restore MBX single bundle",
            "Import MBX single bundle",
            "actions/cache/restore@",
            "actions/cache/save@",
        ] {
            assert!(!yaml.contains(removed), "legacy owner {removed}: {yaml}");
        }
    }
    Ok(())
}
