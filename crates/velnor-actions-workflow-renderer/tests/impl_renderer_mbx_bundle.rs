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
        .map(|offset| action_start + offset)
        .unwrap_or(text.len());
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

    let key_step = &text[key..restore];
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
    let restored = &text[bundle..import];
    assert!(restored.contains("actions/cache/restore@"), "{restored}");
    assert!(
        restored.contains("key: ${{ steps.mbx-cache-key.outputs.key }}"),
        "{restored}"
    );
    assert!(
        restored.contains("restore-keys: ${{ steps.mbx-cache-key.outputs.prefix }}"),
        "{restored}"
    );
    assert_cold_import(&text[import..export]);

    let script = &text[export..save];
    let gc = script.find("mbx gc").expect("GC before export");
    let exported = script.find("cache export").expect("bundle export");
    let durable = script.find("test -d").expect("bundle check");
    let deleted = script.find(r#"rm -rf \"$store\""#).expect("store cleanup");
    assert!(
        gc < exported && exported < durable && durable < deleted,
        "{script}"
    );
    assert!(script.contains("df -B1 -P"), "{script}");
    assert!(script.contains("df -i -P"), "{script}");
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");

    let saved = &text[save..];
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
    Ok(())
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

    let hosted_at = text.find("rust-demo__hosted:").expect("hosted lane");
    let local_at = text.find("rust-demo__local:").expect("Scale Set lane");
    let hosted = &text[hosted_at..local_at];
    let local = &text[local_at..];
    assert!(hosted.contains("backend: github"), "{hosted}");
    assert!(local.contains("backend: local"), "{local}");
    assert!(!local.contains("ACTIONS_CACHE_MODE"), "{local}");
    assert!(!local.contains("cache-primary-key"), "{local}");

    let local_key = local
        .find("name: Prepare MBX cache identity")
        .expect("local cache identity");
    let local_action = local
        .find("name: Prepare MBX local cache store")
        .expect("local action");
    let local_bundle = local
        .find("name: Restore MBX single bundle")
        .expect("bundle restore");
    let local_call = local
        .find("uses: ./.github/actions/rust-demo")
        .expect("shared task action");
    let local_export = local
        .find("name: Export MBX single bundle")
        .expect("bundle export");
    assert!(
        local_key < local_action && local_action < local_bundle && local_bundle < local_call,
        "{local}"
    );
    assert!(local_call < local_export, "{local}");
    let hosted_action = hosted
        .find("name: Restore MBX objects")
        .expect("hosted action");
    let hosted_call = hosted
        .find("uses: ./.github/actions/rust-demo")
        .expect("shared task action");
    assert!(hosted_action < hosted_call, "{hosted}");
    Ok(())
}
