//! The pinned MBX action installs MBX and transports no cache.

use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::shell_step;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn action_ref() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx_job(scale_set: bool) -> Result<String, RenderError> {
    let mut steps = mbx_tool_steps(&action_ref(), TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN)?.to_vec();
    steps.push(shell_step(
        "Read MBX store during build",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "test -n \"$MBX_CACHE_DIR\" && test -d \"$MBX_CACHE_DIR\"".to_owned(),
        ],
        BTreeMap::new(),
    )?);
    let mut built = job("mbx-job", "MBX job", Vec::new(), steps);
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
fn native_action_installs_mbx_and_owns_no_cache_on_both_lanes() -> Result<(), RenderError> {
    for scale_set in [false, true] {
        let yaml = render_mbx_job(scale_set)?;
        assert!(yaml.contains("uses: jdx/mr-boxington-action@"), "{yaml}");
        assert!(yaml.contains("version: 1.21.1"), "{yaml}");
        assert!(yaml.contains("backend: local"), "{yaml}");
        assert!(yaml.contains("cache-links: \"false\""), "{yaml}");
        for key in [
            "MBX_GC_AUTO",
            "MBX_SHARE_OUT_DIR",
            "MBX_CC",
            "MBX_BUILD_SCRIPT_EXECUTION",
            "MBX_TARGET_VIEWS",
        ] {
            assert!(yaml.contains(&format!("{key}: \"0\"")), "{key}: {yaml}");
        }
        assert_eq!(
            yaml.matches("MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx")
                .count(),
            3,
            "preflight, action, and version check use the same step-level path: {yaml}"
        );
        assert!(yaml.contains("GITHUB_ENV"), "job-wide path export: {yaml}");
        assert!(
            !yaml.contains("\n      MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx\n"),
            "runner.temp must never appear in jobs.<id>.env: {yaml}"
        );
        assert!(
            yaml.contains("Read MBX store during build")
                && yaml.contains("test -n \\\"$MBX_CACHE_DIR\\\""),
            "a later build step inherits the preflight-exported store: {yaml}"
        );
        for removed in [
            "github-cache-mode",
            "cache-generation",
            "cache-key",
            "restore-keys",
            "runner.environment",
            "Clean MBX workspace outputs",
            "mbx-bundle",
            "MBX_BUNDLE",
            "actions/cache/restore@",
            "actions/cache/save@",
        ] {
            assert!(!yaml.contains(removed), "retired owner {removed}: {yaml}");
        }
    }
    Ok(())
}
