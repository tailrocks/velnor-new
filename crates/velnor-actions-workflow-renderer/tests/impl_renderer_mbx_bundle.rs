//! Hosted jobs use action-owned isolated caches; Scale Set jobs keep the bundle route.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::steps::mbx_objects_step;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir, shell_step};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

pub(super) fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let mbx = mbx_objects_step(&mbx_uses(), false, "1.21.1")?;
    let build = shell_step(
        "Compile MBX probe",
        vec!["bash".to_owned(), "-c".to_owned(), "mbx build".to_owned()],
        std::collections::BTreeMap::new(),
    )?;
    let mut built = job(id, "MBX job", Vec::new(), vec![mbx, build]);
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

fn position(text: &str, needle: &str) -> usize {
    let position = text.find(needle);
    assert!(position.is_some(), "missing {needle}: {text}");
    position.unwrap_or_default()
}

fn step_block<'a>(text: &'a str, name: &str) -> &'a str {
    let start = position(text, &format!("name: {name}"));
    let tail = &text[start..];
    let end = tail.find("\n      - name:").unwrap_or(tail.len());
    &tail[..end]
}

fn step_blocks<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("name: {name}");
    text.match_indices(&needle)
        .map(|(start, _)| {
            let tail = &text[start..];
            let end = tail.find("\n      - name:").unwrap_or(tail.len());
            &tail[..end]
        })
        .collect()
}

fn assert_hosted_action_options(text: &str) {
    assert!(
        text.contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' }}"),
        "only hosted runners enable action isolation:\n{text}"
    );
    assert!(
        text.contains(
            "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}"
        ),
        "{text}"
    );
    let action = step_block(text, "Restore MBX objects");
    let action_sha = "a".repeat(40);
    let expected_generation = format!(
        "cache-generation: {}-share-out-dir-disabled-v1-action-{action_sha}",
        mbx_cache_generation("1.21.1")
    );
    assert!(action.contains(&expected_generation), "{action}");
    assert!(action.contains("version: 1.21.1"), "{action}");
    assert!(
        !action.contains("cache-key:") && !action.contains("restore-keys:"),
        "keep the action's generated primary-key format and shared compatible restore prefix: {action}"
    );
    assert!(text.contains(&expected_generation), "{text}");
    assert!(text.contains("version: 1.21.1"), "{text}");
    let job_env = position(text, "MBX_SHARE_OUT_DIR: \"0\"");
    let action_step = position(text, "name: Restore MBX objects");
    assert!(
        job_env < action_step,
        "job env must reach the action main and post steps: {text}"
    );
    assert!(text.contains("MBX_GC_AUTO: \"0\""), "{text}");
    let build = step_block(text, "Compile MBX probe");
    assert!(build.contains("run: mbx build"), "{build}");
}

fn assert_hosted_writer_policy(text: &str) {
    assert!(
        text.contains("github.event_name == 'push'"),
        "push is the only writer event:\n{text}"
    );
    assert!(
        text.contains(
            "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"
        ),
        "writes must target the repository default branch:\n{text}"
    );
    assert!(
        text.contains("github.ref_protected == true"),
        "writes require a protected ref:\n{text}"
    );
    assert!(
        text.contains("runner.environment == 'github-hosted' && github.event_name == 'push'"),
        "hosted jobs alone may write:\n{text}"
    );
    assert!(
        text.contains("'write' || 'read'"),
        "all other events stay read-only:\n{text}"
    );
}

fn assert_scale_set_steps_guarded(text: &str) {
    for scale_set_only_step in [
        "Prepare MBX bundle key",
        "Restore MBX single bundle",
        "Import MBX single bundle",
        "Export MBX single bundle",
        "Save MBX single bundle",
        "Verify MBX bundle publication",
        "Collect MBX cache after bundle publication",
    ] {
        assert!(
            step_block(text, scale_set_only_step).contains("runner.environment != 'github-hosted'"),
            "non-hosted bundle step lacks runner guard: {scale_set_only_step}: {text}"
        );
    }
}

fn assert_gc_waits_for_bundle_publication(text: &str) {
    let bundle_export = position(text, "name: Export MBX single bundle");
    let save = position(text, "name: Save MBX single bundle");
    let verify = position(text, "name: Verify MBX bundle publication");
    let gc = position(text, "name: Collect MBX cache after bundle publication");
    assert!(
        bundle_export < save && save < verify && verify < gc,
        "{text}"
    );
    assert!(
        !text.contains("Collect MBX cache before export"),
        "no collector may run before the hosted action exports its receipt closure:\n{text}"
    );
    assert!(
        !step_block(text, "Export MBX single bundle").contains("mbx gc"),
        "the Scale Set exporter must preserve receipt objects until serialization: {text}"
    );
    let verify_block = step_block(text, "Verify MBX bundle publication");
    assert!(
        verify_block.contains("lookup-only: \"true\""),
        "{verify_block}"
    );
    assert!(
        verify_block.contains("key: ${{ steps.mbx.outputs.cache-primary-key }}")
            && !verify_block.contains("restore-keys:"),
        "publication requires an exact lookup of the exported key: {verify_block}"
    );
    assert!(
        verify_block.contains("steps.mbx-export.outputs.ready == 'true'")
            && verify_block.contains("steps.mbx-bundle.outputs.cache-hit != 'true'"),
        "only a cold writer with a nonempty export checks publication: {verify_block}"
    );
    let gc_block = step_block(text, "Collect MBX cache after bundle publication");
    assert!(
        gc_block.contains("steps.mbx-bundle-published.outputs.cache-hit == 'true'"),
        "persistent GC requires an exact published-key hit: {gc_block}"
    );
    assert!(
        !text.contains("Normalize MBX directories before action post"),
        "{text}"
    );
    let share_out_dir = position(text, "MBX_SHARE_OUT_DIR: \"0\"");
    let exporter = position(text, "name: Export MBX single bundle");
    assert!(
        share_out_dir < exporter,
        "job env must reach the Scale Set exporter: {text}"
    );
    assert!(!text.contains("continue-on-error"), "{text}");
}

#[test]
fn hosted_jobs_isolate_each_store_and_write_only_on_protected_default_push()
-> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    assert_hosted_action_options(&text);
    assert_hosted_writer_policy(&text);
    assert_scale_set_steps_guarded(&text);
    assert_gc_waits_for_bundle_publication(&text);
    Ok(())
}

#[test]
fn hosted_jobs_use_job_scoped_primary_keys_with_the_default_restore_prefix()
-> Result<(), RenderError> {
    let uses = mbx_uses();
    let first = mbx_objects_step(&uses, false, "1.21.1")?;
    let second = mbx_objects_step(&uses, false, "1.21.1")?;
    let text = render_workflow_ir(
        &fixture_ir(vec![
            job("verify-alpha", "Verify alpha", Vec::new(), vec![first]),
            job("verify-beta", "Verify beta", Vec::new(), vec![second]),
        ]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let suffix =
        "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}";
    assert_eq!(
        text.matches("uses: jdx/mr-boxington-action@").count(),
        2,
        "{text}"
    );
    assert_eq!(text.matches(suffix).count(), 2, "{text}");
    assert!(text.contains("verify-alpha:"), "{text}");
    assert!(text.contains("verify-beta:"), "{text}");
    let actions = step_blocks(&text, "Restore MBX objects");
    assert_eq!(actions.len(), 2, "{text}");
    for action in actions {
        assert!(
            !action.contains("cache-key:") && !action.contains("restore-keys:"),
            "preserve the action's generated primary key and compatible restore prefix: {action}"
        );
    }
    Ok(())
}

fn assert_external_import_fallback(imported: &str) {
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

fn assert_scale_set_action_policy(action: &str) {
    assert!(action.contains("id: mbx"), "{action}");
    assert!(
        action.contains(
            "ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted' && github.event_name == 'push'"
        ),
        "the self-hosted branch resolves read-only:\n{action}"
    );
    assert!(
        action.contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' }}"),
        "isolation is disabled on Scale Set runners:\n{action}"
    );
    assert!(
        action.contains(
            "cache-key-suffix: ${{ runner.environment == 'github-hosted' && github.job || '' }}"
        ),
        "Scale Set key suffix resolves empty:\n{action}"
    );
    assert!(
        !action.contains("cache-key:") && !action.contains("restore-keys:"),
        "Scale Set retains the generated cache key and restore-prefix policy:\n{action}"
    );
}

fn assert_scale_set_bundle_save(saved: &str) {
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
        saved.contains("runner.environment != 'github-hosted'"),
        "{saved}"
    );
    assert!(!saved.contains("pull_request"), "{saved}");
}

#[test]
fn scale_set_jobs_keep_the_existing_single_bundle_lifecycle() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    let restore = position(&text, "name: Restore MBX objects");
    let bundle_key = position(&text, "name: Prepare MBX bundle key");
    let bundle = position(&text, "name: Restore MBX single bundle");
    let import = position(&text, "name: Import MBX single bundle");
    let export = position(&text, "name: Export MBX single bundle");
    let save = position(&text, "name: Save MBX single bundle");
    let verify = position(&text, "name: Verify MBX bundle publication");
    let gc = position(&text, "name: Collect MBX cache after bundle publication");
    assert!(
        restore < bundle_key
            && bundle_key < bundle
            && bundle < import
            && import < export
            && export < save
            && save < verify
            && verify < gc,
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
    assert_external_import_fallback(&text[import..export]);
    assert_scale_set_action_policy(&text[restore..bundle_key]);
    let saved = &text[save..];
    assert_scale_set_bundle_save(saved);
    assert_gc_waits_for_bundle_publication(&text);
    assert!(
        !text.contains("Normalize MBX directories before action post"),
        "{text}"
    );
    assert!(!text.contains("continue-on-error"), "{text}");
    Ok(())
}
