//! Hosted MBX saves one bundle outside the store. The action post does not.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let mbx = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let mut built = job(id, "MBX job", Vec::new(), mbx.into());
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

fn expected_generation() -> String {
    let action_sha = &mbx_uses()["jdx/mr-boxington-action@".len()..];
    format!(
        "cache-generation: {}${{{{ runner.environment == 'github-hosted' && runner.os == 'Linux' && '-share-out-dir-disabled-v1' || '' }}}}-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    )
}

fn assert_hosted_cache_policy(text: &str, action: &str, restore: usize) {
    assert!(action.contains("id: mbx"), "{action}");
    assert!(
        action.contains("ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted'"),
        "cache writes stay runner-gated: {action}"
    );
    assert!(
        action.contains("github.ref_protected == true && 'write' || 'read'"),
        "only protected default-branch pushes write: {action}"
    );
    assert!(
        action.contains(
            "isolate-objects-cache: ${{ runner.environment == 'github-hosted' && runner.os == 'Linux' }}"
        ),
        "{action}"
    );
    assert!(
        action.contains(
            "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}"
        ),
        "{action}"
    );
    assert!(action.contains(&expected_generation()), "{action}");
    let policy = text.find("MBX_SHARE_OUT_DIR: \"0\"").expect("share env");
    assert!(policy < restore, "job env precedes restore:\n{text}");
    assert!(text.contains("MBX_GC_AUTO: \"0\""), "{text}");
}

fn assert_bundle_save(saved: &str) {
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
    assert!(
        saved.contains("if: runner.environment != 'github-hosted' && success()"),
        "bundle save stays on the Scale Set route: {saved}"
    );
    assert!(!saved.contains("pull_request"), "{saved}");
}

fn assert_cold_import(imported: &str) {
    for needle in [
        "mbx cache import",
        "no mbx bundle matched",
        "mbx bundle missing; continuing cold",
        "mbx bundle import failed; continuing cold",
        "df -B1 -P",
        "df -i -P",
    ] {
        assert!(
            imported.contains(needle),
            "{needle} missing from {imported}"
        );
    }
    assert!(!imported.contains("test -d"), "{imported}");
}

#[test]
fn hosted_export_samples_disk_around_store_delete() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let export = text
        .find("name: Export MBX single bundle")
        .expect("export step");
    let save = text
        .find("name: Save MBX single bundle")
        .expect("save step");
    let script = &text[export..save];
    let bytes = script.find("df -B1 -P").expect("byte sample");
    let inodes = script.find("df -i -P").expect("inode sample");
    let gc = script.find("mbx gc").expect("reclaim");
    let exported = script.find("cache export").expect("export");
    let durable = script.find("test -d").expect("bundle check");
    let deleted = script.find(r#"rm -rf \"$store\""#).expect("store delete");
    assert!(bytes < inodes, "{script}");
    assert!(inodes < gc, "{script}");
    assert!(gc < exported, "{script}");
    assert!(
        script.contains("if: runner.environment != 'github-hosted'"),
        "bundle export stays on the Scale Set route: {script}"
    );
    assert!(exported < durable, "{script}");
    assert!(durable < deleted, "{script}");
    let after = &script[deleted..];
    assert!(after.contains("df -B1 -P"), "{after}");
    assert!(after.contains("df -i -P"), "{after}");
    assert!(
        script.contains("$RUNNER_TEMP/mbx-single-bundle"),
        "{script}"
    );
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");
    assert!(!script.contains("github-actions-cache-v1"), "{script}");
    Ok(())
}

#[test]
fn hosted_isolation_keeps_scale_set_bundle_guarded() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let preflight = text
        .find("name: Verify MBX and Rust toolchains")
        .expect("toolchain preflight");
    let restore = text
        .find("name: Restore MBX objects")
        .expect("restore step");
    let bundle_key = text
        .find("name: Prepare MBX bundle key")
        .expect("bundle key");
    let bundle = text
        .find("name: Restore MBX single bundle")
        .expect("bundle restore");
    let import = text
        .find("name: Import MBX single bundle")
        .expect("bundle import");
    let export = text
        .find("name: Export MBX single bundle")
        .expect("export step");
    let save = text
        .find("name: Save MBX single bundle")
        .expect("save step");
    assert!(
        preflight < restore
            && restore < bundle_key
            && bundle_key < bundle
            && bundle < import
            && import < export
            && export < save,
        "{text}"
    );
    let restored = &text[bundle..import];
    assert!(restored.contains("actions/cache/restore@"), "{restored}");
    assert!(
        restored.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{restored}"
    );
    assert!(
        restored.contains("restore-keys: ${{ steps.mbx-bundle-key.outputs.prefix }}"),
        "{restored}"
    );
    assert_cold_import(&text[import..export]);
    assert_hosted_cache_policy(&text, &text[restore..export], restore);
    assert_bundle_save(&text[save..]);
    assert!(!text.contains("continue-on-error"), "{text}");
    Ok(())
}

#[test]
fn scale_set_save_matches_and_skips_hosted_gc_env() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    assert!(text.contains("name: Export MBX single bundle"), "{text}");
    let mode = text.find("ACTIONS_CACHE_MODE:").expect("cache mode");
    let mode_line = text[mode..].lines().next().expect("mode line");
    assert!(
        mode_line.contains("|| 'none'"),
        "non-hosted cache mode is none: {mode_line}"
    );
    let restore = text
        .find("name: Restore MBX single bundle")
        .expect("bundle restore");
    let import = text
        .find("name: Import MBX single bundle")
        .expect("bundle import");
    assert!(
        text[restore..import].contains("if: runner.environment != 'github-hosted'"),
        "bundle restore stays on the Scale Set route: {text}"
    );
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    assert!(!text.contains("MBX_SHARE_OUT_DIR"), "{text}");
    assert!(text.contains(&expected_generation()), "{text}");
    Ok(())
}
