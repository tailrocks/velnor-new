//! The pinned MBX v1.6 action and Velnor use disjoint cache lanes.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

const ACTION_V1_6: &str = "1687e54eb349cadf61fa38b5813a77875489e8e6";
const HOSTED_BACKEND: &str =
    "backend: ${{ runner.environment == 'github-hosted' && 'github' || 'local' }}";
const HOSTED_MODE: &str = "ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted' && github.event_name == 'push' && 'write' || 'read' }}";

fn render_lane(scale_set: bool) -> Result<String, RenderError> {
    let uses = format!("jdx/mr-boxington-action@{ACTION_V1_6}");
    let mbx = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let mut built = job("rust-demo", "MBX job", Vec::new(), mbx.into());
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
fn actual_v16_emitter_selects_one_cache_owner_per_lane() -> Result<(), RenderError> {
    for scale_set in [false, true] {
        let text = render_lane(scale_set)?;
        let action = text
            .find("name: Restore MBX objects")
            .expect("MBX action is emitted");
        let key = text
            .find("name: Prepare MBX bundle key")
            .expect("Velnor key step is emitted");
        let action_block = &text[action..key];
        assert!(action_block.contains(ACTION_V1_6), "{action_block}");
        assert!(action_block.contains(HOSTED_BACKEND), "{action_block}");
        assert!(action_block.contains(HOSTED_MODE), "{action_block}");
        assert!(
            !action_block.contains("isolate-objects-cache"),
            "{action_block}"
        );

        let key = text
            .find("name: Prepare MBX bundle key")
            .expect("MBX transport key step");
        let restore = text
            .find("name: Restore MBX single bundle")
            .expect("MBX cache restore step");
        let key_block = &text[key..restore];
        assert!(
            key_block.contains("MBX_GENERATION: velnor-mbx-1.21.1-dir"),
            "the outer key must use the pinned action's directory generation: {key_block}"
        );
        assert!(
            key_block.contains("MBX_TOOLCHAIN: 1.98.1"),
            "the outer key must use the exact action toolchain: {key_block}"
        );
        assert!(
            key_block.contains("continue-on-error: true"),
            "key output handoff failure must preserve the build: {key_block}"
        );
        assert!(
            !key_block.contains("steps.mbx.outputs.cache-primary-key"),
            "v1.6 local backend does not publish this output: {key_block}"
        );

        for name in [
            "Initialize private MBX store",
            "Prepare MBX bundle key",
            "Restore MBX single bundle",
            "Import MBX single bundle",
            "Export MBX single bundle",
            "Save MBX single bundle",
        ] {
            let start = text
                .find(&format!("name: {name}"))
                .expect("generated MBX step is missing");
            let next = text[start + 1..]
                .find("\n      - name:")
                .map(|offset| start + 1 + offset)
                .unwrap_or(text.len());
            let block = &text[start..next];
            assert!(
                block.contains("if: runner.environment != 'github-hosted'"),
                "hosted action must be the sole hosted cache owner: {block}"
            );
        }
        let import = text
            .find("name: Import MBX single bundle")
            .expect("MBX import step");
        let export = text
            .find("name: Export MBX single bundle")
            .expect("MBX export step");
        let save = text
            .find("name: Save MBX single bundle")
            .expect("MBX cache save step");
        let restore_end = text[restore + 1..]
            .find("\n      - name:")
            .map(|offset| restore + 1 + offset)
            .unwrap_or(text.len());
        let restore_block = &text[restore..restore_end];
        let save_block = &text[save..];
        let restore_path = restore_block
            .lines()
            .find(|line| line.trim_start().starts_with("path:"))
            .expect("restore path input");
        let save_path = save_block
            .lines()
            .find(|line| line.trim_start().starts_with("path:"))
            .expect("save path input");
        assert_eq!(restore_path, save_path, "cache version paths must match");
        assert!(
            restore_block.contains("steps.mbx-bundle-key.outcome == 'success'"),
            "restore requires a successful key handoff: {restore_block}"
        );
        assert!(
            restore_block.contains("continue-on-error: true"),
            "a failed cache restore must not fail the build: {restore_block}"
        );
        assert!(
            restore_path.contains("${{ runner.temp }}/mbx-single-bundle"),
            "{restore_path}"
        );
        assert!(
            text[import..export].contains("continue-on-error: true"),
            "an import handoff failure must preserve the build: {}",
            &text[import..export]
        );
        assert!(
            text[import..export].contains("steps.mbx-bundle-key.outcome == 'success'"),
            "import must require the key handoff and stay Scale Set-only: {}",
            &text[import..export]
        );
        assert!(
            text[import..export].contains("steps.mbx-bundle.outcome == 'success'"),
            "import must not run after a failed restore: {}",
            &text[import..export]
        );
        let save = text
            .find("name: Save MBX single bundle")
            .expect("MBX save step");
        assert!(
            text[export..save].contains("continue-on-error: true"),
            "an export handoff failure must preserve the build: {}",
            &text[export..save]
        );
        assert!(
            text[export..save].contains("steps.mbx-import.outcome == 'success'"),
            "export requires a successful import handoff"
        );
        assert!(
            save_block.contains("steps.mbx-export.outcome == 'success'"),
            "save requires a successful export handoff: {save_block}"
        );
        assert!(
            save_block.starts_with("name: Save MBX single bundle")
                && save_block.contains("continue-on-error: true"),
            "cache-save failure must not fail the build: {save_block}"
        );
    }
    Ok(())
}
