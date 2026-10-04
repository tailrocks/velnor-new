//! Hosted MBX saves one bundle outside the store. The action post does not.

use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::render::{
    RenderedWorkflow, render_workflow_ir_strict_shared,
};
use velnor_actions_workflow_renderer::steps::checkout_step;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir, shell_step};

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

fn render_plain() -> Result<RenderedWorkflow, RenderError> {
    let plain = job(
        "plain",
        "Plain job",
        Vec::new(),
        vec![shell_step(
            "No MBX",
            vec!["printf".to_owned(), "ok".to_owned()],
            BTreeMap::new(),
        )?],
    );
    render_workflow_ir_strict_shared(
        &fixture_ir(vec![plain]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
}

fn assert_local_store_fallback(imported: &str) {
    for needle in [
        "mbx cache import",
        "no mbx bundle matched; using local store",
        "mbx bundle missing; using local store",
        "mbx bundle import failed; using local store",
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

fn assert_hosted_cache_policy(text: &str, action: &str) {
    assert!(action.contains("id: mbx"), "{action}");
    assert!(
        action.contains("ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted'"),
        "cache writes stay runner-gated: {action}"
    );
    assert!(
        action.contains("github.ref_protected == true && 'write' || 'read' }}"),
        "only protected default-branch pushes write: {action}"
    );
    assert!(
        matches!(
            (
                text.find("MBX_SHARE_OUT_DIR: \"0\""),
                text.find("name: Restore MBX objects")
            ),
            (Some(policy_env), Some(restore)) if policy_env < restore
        ),
        "hosted Linux job env precedes action main and post: {text}"
    );
    assert!(text.contains("MBX_GC_AUTO: \"0\""), "{text}");
    let action_uses = mbx_uses();
    let action_sha = &action_uses["jdx/mr-boxington-action@".len()..];
    let expected_generation = format!(
        "cache-generation: {}${{{{ runner.environment == 'github-hosted' && runner.os == 'Linux' && '-share-out-dir-disabled-v1' || '' }}}}-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    );
    assert!(action.contains(&expected_generation), "{action}");
}

fn assert_bundle_save_policy(saved: &str) {
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
}

#[test]
fn scale_set_export_collects_before_export_and_store_delete() -> Result<(), RenderError> {
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
        "collection remains confined to the Scale Set route: {script}"
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
    assert_local_store_fallback(&text[import..export]);
    let action = &text[restore..export];
    assert_hosted_cache_policy(&text, action);
    let saved = &text[save..];
    assert_bundle_save_policy(saved);
    assert!(!text.contains("continue-on-error"), "{text}");
    Ok(())
}

fn hosted_prune_fixture() -> Result<RenderedWorkflow, RenderError> {
    let [preflight, mbx] = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let report_upload = shell_step(
        "Upload crate reports",
        vec!["bash".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )?;
    let source_save = shell_step(
        "Save Cargo sources",
        vec!["bash".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )?;
    let tools_save = shell_step(
        "Save Mise tools",
        vec!["bash".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )?;
    let checkout = checkout_step(&checkout_pin())?;
    let steps = vec![
        checkout.clone(),
        preflight.clone(),
        mbx.clone(),
        report_upload,
        source_save,
        tools_save,
    ];
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        steps.clone(),
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        steps,
    );
    local.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?
    .token();
    render_workflow_ir_strict_shared(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
}

fn assert_shared_report_save_order(rendered: &RenderedWorkflow) -> Result<(), RenderError> {
    let composite = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/rust-demo/action.yml")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_shared_lane".to_owned()))?;
    let composite = composite.bytes.as_str();
    let report = composite
        .find("name: Upload crate reports")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_report_step".to_owned()))?;
    let source = composite
        .find("name: Save Cargo sources")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_source_save_step".to_owned()))?;
    assert!(
        report < source,
        "report and source save order in composite:\n{composite}"
    );
    Ok(())
}

fn assert_hosted_prune_order_and_scope(text: &str) -> Result<(), RenderError> {
    let hosted_start = text
        .find("rust-demo__hosted:")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_hosted_job".to_owned()))?;
    let local_start = text
        .find("rust-demo__local:")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_scale_set_job".to_owned()))?;
    let hosted_section = &text[hosted_start..local_start];
    let composite_call = hosted_section
        .find("uses: ./.github/actions/rust-demo")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_shared_call".to_owned()))?;
    let tools = hosted_section
        .find("name: Save Mise tools")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_elected_save".to_owned()))?;
    let prune = hosted_section
        .find("name: Measure and prune Cargo sources")
        .ok_or_else(|| RenderError::InvalidWorkflow("test_missing_source_prune".to_owned()))?;
    assert!(composite_call < tools && tools < prune, "{hosted_section}");
    assert!(
        !text[local_start..].contains("Measure and prune Cargo sources"),
        "Scale Set must not prune its persistent lane:\n{text}"
    );
    let cleanup = &hosted_section[prune..];
    for needle in [
        "if: success() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && runner.environment == 'github-hosted' && runner.os == 'Linux'",
        "CARGO_HOME: ${{ runner.temp }}/velnor/cargo",
        "python3 \\\"$GITHUB_WORKSPACE/.github/scripts/prune_hosted_cargo_sources.py\\\"",
    ] {
        assert!(
            cleanup.contains(needle),
            "missing `{needle}` in cleanup:\n{cleanup}"
        );
    }
    Ok(())
}

#[test]
fn hosted_linux_prunes_only_cargo_sources_after_reports_and_saves() -> Result<(), RenderError> {
    let rendered = hosted_prune_fixture()?;
    assert_shared_report_save_order(&rendered)?;
    assert_hosted_prune_order_and_scope(&rendered.yaml)?;
    let helper = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/scripts/prune_hosted_cargo_sources.py")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_generated_source_prune".to_owned()))?;
    assert!(
        helper.bytes.contains("def prune_sources("),
        "{}",
        helper.bytes
    );
    assert!(helper.bytes.contains("mnt_id:"), "{}", helper.bytes);
    Ok(())
}

#[test]
fn no_hosted_linux_mbx_omits_the_prune_step_and_helper() -> Result<(), RenderError> {
    let rendered = render_plain()?;
    assert!(!rendered.yaml.contains("Measure and prune Cargo sources"));
    assert!(
        !rendered
            .shared
            .iter()
            .any(|file| file.path == ".github/scripts/prune_hosted_cargo_sources.py")
    );
    Ok(())
}

#[test]
fn scale_set_save_matches_and_skips_hosted_gc_env() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    assert!(text.contains("name: Export MBX single bundle"), "{text}");
    assert!(
        text.contains("ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted'"),
        "Scale Set action resolves to its existing read-only policy: {text}"
    );
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    assert!(!text.contains("MBX_SHARE_OUT_DIR"), "{text}");
    assert!(!text.contains("Measure and prune Cargo sources"), "{text}");
    let action_uses = mbx_uses();
    let action_sha = &action_uses["jdx/mr-boxington-action@".len()..];
    let expected_generation = format!(
        "cache-generation: {}${{{{ runner.environment == 'github-hosted' && runner.os == 'Linux' && '-share-out-dir-disabled-v1' || '' }}}}-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    );
    assert!(text.contains(&expected_generation), "{text}");
    Ok(())
}
