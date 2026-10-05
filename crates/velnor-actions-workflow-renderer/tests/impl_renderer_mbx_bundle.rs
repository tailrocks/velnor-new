//! One explicit stock bundle lifecycle around every MBX producer.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir, steps::checkout_step};

use super::impl_renderer_fixtures::*;

const LOCAL_SETUP: &str = "name: Prepare MBX local cache store";
const KEY_STEP: &str = "name: Prepare MBX cache identity";
const RESTORE: &str = "name: Restore MBX single bundle";
const IMPORT: &str = "name: Import MBX single bundle";
const EXPORT: &str = "name: Export MBX single bundle";
const SAVE: &str = "name: Save MBX single bundle";
const CLEANUP: &str = "name: Cleanup MBX single bundle";

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let pair = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let mut steps = pair.into_iter().collect::<Vec<_>>();
    steps.push(scrubbed_shell_step(
        "Build Rust producer",
        vec!["mbx".to_owned(), "build".to_owned()],
    )?);
    let mut built = job(id, "MBX job", Vec::new(), steps);
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

fn expected_action_generation() -> String {
    let action_sha = &mbx_uses()["jdx/mr-boxington-action@".len()..];
    format!(
        "cache-generation: {}-share-out-dir-disabled-v1-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    )
}

fn assert_one_lifecycle(text: &str) {
    for (needle, count) in [
        (RESTORE, 1),
        (IMPORT, 1),
        (EXPORT, 1),
        (SAVE, 1),
        (CLEANUP, 1),
    ] {
        assert_eq!(text.matches(needle).count(), count, "{needle}: {text}");
    }
    let missing = usize::MAX;
    let preflight = text
        .find("name: Verify MBX and Rust toolchains")
        .unwrap_or(missing);
    let key = text.find(KEY_STEP).unwrap_or(missing);
    let setup = text.find(LOCAL_SETUP).unwrap_or(missing);
    let restore = text.find(RESTORE).unwrap_or(missing);
    let import = text.find(IMPORT).unwrap_or(missing);
    let build = text.find("name: Build Rust producer").unwrap_or(missing);
    let export = text.find(EXPORT).unwrap_or(missing);
    let save = text.find(SAVE).unwrap_or(missing);
    let cleanup = text.find(CLEANUP).unwrap_or(missing);
    assert!(
        preflight < key && key < setup && setup < restore && restore < import,
        "setup/restore order: {text}"
    );
    assert!(
        import < build && build < export && export < save && save < cleanup,
        "producer lifecycle: {text}"
    );
    assert!(text.contains("backend: local"), "{text}");
    assert!(text.contains("github-cache-mode: objects"), "{text}");
    assert!(text.contains("ACTIONS_CACHE_MODE: read"), "{text}");
    assert!(
        text.contains("MBX_CACHE_DIR: ${{ steps.mbx_cache_preflight.outputs.cache_dir }}"),
        "{text}"
    );
    assert!(
        text.contains(
            "MBX_CACHE_EXPORT_GROUP: ${{ steps.mbx_cache_preflight.outputs.export_group }}"
        ),
        "{text}"
    );
    assert!(
        text.contains("path: .velnor-mbx-cache/mbx-single-bundle"),
        "{text}"
    );
    assert!(
        text.contains("restore-keys: ${{ steps.mbx-cache-key.outputs.prefix }}"),
        "{text}"
    );
    assert!(
        text.contains("key: ${{ steps.mbx-cache-key.outputs.key }}"),
        "{text}"
    );
    assert!(text.contains(&expected_action_generation()), "{text}");
    assert!(text.contains("github.event_name == 'push'"), "{text}");
    assert!(
        text.contains("toJSON(github.event.repository.fork) == 'false'"),
        "{text}"
    );
    assert!(
        text.contains("github.event.repository.default_branch != ''"),
        "{text}"
    );
    assert!(text.contains("github.ref_protected == true"), "{text}");
    assert!(!text.contains("cache-key-suffix"), "{text}");
    assert!(!text.contains("isolate-objects-cache"), "{text}");
    assert!(!text.contains("mbx gc"), "{text}");
    assert!(!text.contains("rm -rf"), "{text}");
}

#[test]
fn hosted_and_scale_set_jobs_use_one_explicit_lifecycle() -> Result<(), RenderError> {
    let hosted = render_mbx("hosted", false)?;
    assert_one_lifecycle(&hosted);
    assert!(hosted.contains("MBX_GC_AUTO: \"0\""), "{hosted}");
    assert!(hosted.contains("MBX_SHARE_OUT_DIR: \"0\""), "{hosted}");
    assert!(
        hosted.contains("RUNNER_LANE: hosted-ubuntu-26.04"),
        "{hosted}"
    );

    let scale = render_mbx("scale-local", true)?;
    assert_one_lifecycle(&scale);
    assert!(!scale.contains("MBX_GC_AUTO"), "{scale}");
    assert!(scale.contains("MBX_SHARE_OUT_DIR: \"0\""), "{scale}");
    assert!(
        scale.contains("RUNNER_LANE: scale-set-ubuntu-26.04-scale-set"),
        "{scale}"
    );
    Ok(())
}

#[test]
fn paired_lanes_keep_restore_before_composite_and_save_after_it() -> Result<(), RenderError> {
    let action = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let checkout = checkout_step(&checkout_pin())?;
    let build = scrubbed_shell_step(
        "Build Rust producer",
        vec!["mbx".to_owned(), "build".to_owned()],
    )?;
    let steps = vec![checkout, action[0].clone(), action[1].clone(), build];
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        steps.clone(),
    );
    let mut scale = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo Scale Set",
        Vec::new(),
        steps,
    );
    scale.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?
    .token();
    let text = render_workflow_ir(
        &fixture_ir(vec![hosted, scale]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches(RESTORE).count(), 2, "{text}");
    assert_eq!(text.matches(IMPORT).count(), 2, "{text}");
    assert_eq!(text.matches(EXPORT).count(), 2, "{text}");
    assert_eq!(text.matches(SAVE).count(), 2, "{text}");
    for lane in ["rust-demo__hosted:", "rust-demo__local:"] {
        let start = text.find(lane).expect("lane");
        let tail = &text[start..];
        let call = tail
            .find("uses: ./.github/actions/rust-demo")
            .expect("composite");
        let import = tail.find(IMPORT).expect("import");
        let export = tail.find(EXPORT).expect("export");
        let save = tail.find(SAVE).expect("save");
        assert!(
            import < call && call < export && export < save,
            "{lane}: {tail}"
        );
    }
    Ok(())
}
