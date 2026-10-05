//! Typed MBX backend routes and their separate Scale Set bundle protocol.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir, steps::checkout_step};

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

fn expected_action_generation() -> String {
    let action_sha = &mbx_uses()["jdx/mr-boxington-action@".len()..];
    format!(
        "cache-generation: {}-share-out-dir-disabled-v1-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    )
}

fn assert_cold_import(imported: &str) {
    for needle in [
        "mbx cache import",
        "no mbx bundle matched",
        "mbx bundle missing; continuing cold",
        "mbx bundle import failed; continuing cold",
        "df -B1 -P",
        "df -i -P",
        "/opt/velnor/seed/mbx",
        "mbx-seed-bundle",
        "steps.mbx-cache-key.outputs.prefix",
    ] {
        assert!(
            imported.contains(needle),
            "{needle} missing from {imported}"
        );
    }
    assert!(!imported.contains("test -d"), "{imported}");
}

#[test]
fn hosted_jobs_keep_the_action_owned_object_cache() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let action_start = text.find("name: Restore MBX objects").expect("restore");
    let action_end = text[action_start..]
        .find("\n      - name:")
        .map_or(text.len(), |offset| action_start + offset);
    let action = &text[action_start..action_end];
    assert!(action.contains("backend: github"), "{action}");
    assert!(action.contains("github-cache-mode: objects"), "{action}");
    assert!(
        action.contains("ACTIONS_CACHE_MODE: ${{ github.event_name == 'push'"),
        "protected default-branch writes only: {action}"
    );
    assert!(
        action.contains("github.ref_protected == true && 'write' || 'read'"),
        "{action}"
    );
    assert!(
        action.contains("cache-key: ${{ steps.mbx-cache-key.outputs.key }}"),
        "{action}"
    );
    assert!(
        action.contains("restore-keys: ${{ steps.mbx-cache-key.outputs.prefix }}"),
        "{action}"
    );
    assert!(!action.contains("isolate-objects-cache"), "{action}");
    assert!(!action.contains("cache-key-suffix"), "{action}");
    assert!(action.contains(&expected_action_generation()), "{action}");
    assert!(text.contains("MBX_GC_AUTO: \"0\""), "{text}");
    assert!(text.contains("MBX_SHARE_OUT_DIR: \"0\""), "{text}");
    assert!(text.contains("name: Prepare MBX cache identity"), "{text}");
    for bundle_step in [
        "Restore MBX single bundle",
        "Import MBX single bundle",
        "Export MBX single bundle",
        "Save MBX single bundle",
    ] {
        assert!(
            !text.contains(bundle_step),
            "hosted route emitted {bundle_step}: {text}"
        );
    }
    Ok(())
}

#[test]
fn scale_set_owns_key_group_restore_import_export_and_save() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    let preflight = text
        .find("name: Verify MBX and Rust toolchains")
        .expect("toolchain preflight");
    let restore = text
        .find("name: Prepare MBX local cache store")
        .expect("local backend setup");
    let key = text
        .find("name: Prepare MBX cache identity")
        .expect("cache identity");
    let bundle = text
        .find("name: Restore MBX single bundle")
        .expect("bundle restore");
    let import = text
        .find("name: Import MBX single bundle")
        .expect("bundle import");
    let export = text
        .find("name: Export MBX single bundle")
        .expect("bundle export");
    let save = text
        .find("name: Save MBX single bundle")
        .expect("bundle save");
    assert!(
        preflight < key && key < restore && restore < bundle,
        "{text}"
    );
    assert!(
        bundle < import && import < export && export < save,
        "{text}"
    );
    assert!(
        text.contains("backend: local"),
        "Scale Set avoids action restore: {text}"
    );
    assert!(!text.contains("ACTIONS_CACHE_MODE"), "{text}");
    assert!(!text.contains("cache-primary-key"), "{text}");
    assert!(text.contains("MBX_SHARE_OUT_DIR: \"0\""), "{text}");
    assert!(
        !text.contains("MBX_GC_AUTO"),
        "Scale Set keeps MBX GC enabled: {text}"
    );

    assert_scale_set_key(&text[key..restore]);
    assert_scale_set_restore(&text[bundle..import]);
    assert_cold_import(&text[import..export]);
    assert_scale_set_export(&text[export..save]);
    assert_scale_set_save(&text[save..]);
    Ok(())
}

fn assert_scale_set_key(key_step: &str) {
    for needle in [
        "rustc",
        "RUST_TOOLCHAIN",
        "sha256sum",
        "CACHE_REVISION",
        "github-actions-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${group_id}",
        "${GITHUB_JOB}-${CACHE_REVISION}",
        "CREATE_EXPORT_GROUP",
        "/proc/sys/kernel/random/uuid",
    ] {
        assert!(
            key_step.contains(needle),
            "{needle} missing from {key_step}"
        );
    }
    assert!(
        !key_step.contains("$("),
        "command substitution is forbidden: {key_step}"
    );
}

fn assert_scale_set_restore(restored: &str) {
    assert!(restored.contains("actions/cache/restore@"), "{restored}");
    assert!(
        restored.contains("key: ${{ steps.mbx-cache-key.outputs.key }}"),
        "{restored}"
    );
    assert!(
        restored.contains("restore-keys: ${{ steps.mbx-cache-key.outputs.prefix }}"),
        "{restored}"
    );
}

fn assert_scale_set_export(script: &str) {
    assert!(
        matches!(
            (
                script.find("mbx gc"),
                script.find("cache export"),
                script.find("test -d"),
                script.find(r#"rm -rf \"$store\""#),
            ),
            (Some(gc), Some(exported), Some(durable), Some(deleted))
                if gc < exported && exported < durable && durable < deleted
        ),
        "GC, export, bundle check, and store cleanup must be ordered: {script}"
    );
    assert!(script.contains("df -B1 -P"), "{script}");
    assert!(script.contains("df -i -P"), "{script}");
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");
}

fn assert_scale_set_save(saved: &str) {
    assert!(
        saved.contains("key: ${{ steps.mbx-cache-key.outputs.key }}"),
        "{saved}"
    );
    assert!(
        saved.contains("steps.mbx-export.outputs.ready == 'true'"),
        "{saved}"
    );
    assert!(saved.contains("github.event_name == 'push'"), "{saved}");
    assert!(
        saved.contains("github.ref_protected == true"),
        "unprotected push cannot save: {saved}"
    );
}

#[test]
fn both_mode_shares_tasks_after_typed_cache_preludes() -> Result<(), RenderError> {
    use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};

    let action = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let checkout = checkout_step(&checkout_pin())?;
    let build = scrubbed_shell_step("Build", vec!["mbx".to_owned(), "build".to_owned()])?;
    let steps = vec![checkout, action[0].clone(), action[1].clone(), build];
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        steps.clone(),
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo Scale Set",
        Vec::new(),
        steps,
    );
    local.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?
    .token();
    let text = render_workflow_ir(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(
        text.matches("uses: ./.github/actions/rust-demo").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("MBX_SHARE_OUT_DIR: \"0\"").count(),
        2,
        "{text}"
    );
    assert_eq!(text.matches("MBX_GC_AUTO: \"0\"").count(), 1, "{text}");
    assert_eq!(
        text.matches("name: Restore MBX single bundle").count(),
        1,
        "{text}"
    );

    assert_shared_lane_routes(&text);
    Ok(())
}

fn assert_shared_lane_routes(text: &str) {
    let hosted_at = must_find(text, "rust-demo__hosted:");
    let local_at = must_find(text, "rust-demo__local:");
    let hosted = &text[hosted_at..local_at];
    let local = &text[local_at..];
    assert!(hosted.contains("backend: github"), "{hosted}");
    assert!(local.contains("backend: local"), "{local}");
    assert!(!local.contains("ACTIONS_CACHE_MODE"), "{local}");
    assert!(!local.contains("cache-primary-key"), "{local}");
    let local_key = must_find(local, "name: Prepare MBX cache identity");
    let local_action = must_find(local, "name: Prepare MBX local cache store");
    let local_bundle = must_find(local, "name: Restore MBX single bundle");
    let local_call = must_find(local, "uses: ./.github/actions/rust-demo");
    let local_export = must_find(local, "name: Export MBX single bundle");
    assert!(
        local_key < local_action && local_action < local_bundle,
        "{local}"
    );
    assert!(
        local_bundle < local_call && local_call < local_export,
        "{local}"
    );
    let hosted_action = must_find(hosted, "name: Restore MBX objects");
    let hosted_call = must_find(hosted, "uses: ./.github/actions/rust-demo");
    assert!(hosted_action < hosted_call, "{hosted}");
}

fn must_find(text: &str, needle: &str) -> usize {
    let found = text.find(needle);
    assert!(found.is_some(), "{needle} missing from {text}");
    found.unwrap_or_default()
}

#[path = "impl_renderer_mbx_bundle_export_test.rs"]
mod export_failure_tests;
