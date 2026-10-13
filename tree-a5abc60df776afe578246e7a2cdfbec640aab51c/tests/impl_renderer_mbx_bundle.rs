//! Hosted MBX saves one bundle outside the store. The action post does not.

use velnor_actions_contract::WorkflowPolicy;
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

fn render_parallel_mbx() -> Result<String, RenderError> {
    let checkout = velnor_actions_workflow_renderer::checkout_step(&checkout_pin())?;
    let mbx = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let hosted = job(
        "rust-demo__hosted",
        "Hosted MBX job",
        Vec::new(),
        vec![checkout.clone(), mbx[0].clone(), mbx[1].clone()],
    );
    let mut local = job(
        "rust-demo__local",
        "Scale Set MBX job",
        Vec::new(),
        vec![checkout, mbx[0].clone(), mbx[1].clone()],
    );
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    local.1.runs_on = selector.token();
    render_workflow_ir(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
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
fn hosted_save_is_one_bundle_outside_the_store() -> Result<(), RenderError> {
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
    let key_step = &text[bundle_key..bundle];
    assert!(
        key_step.contains("MBX_JOB_ID: ${{ github.job }}"),
        "{key_step}"
    );
    assert!(key_step.contains("MBX_MATRIX_KEY: \"\""), "{key_step}");
    assert!(
        key_step.contains("primary=\\\"${key}-${suffix}\\\""),
        "{key_step}"
    );
    assert!(key_step.contains("prefix=\\\"${key%-*}-\\\""), "{key_step}");
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
    let action = &text[restore..export];
    assert!(action.contains("id: mbx"), "{action}");
    assert!(action.contains("ACTIONS_CACHE_MODE: read"), "{action}");
    assert!(!action.contains("write"), "{action}");
    let saved = &text[save..];
    assert!(
        saved.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{saved}"
    );
    assert!(
        saved.contains("key: ${{ steps.mbx-bundle-key.outputs.key }}"),
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
fn parallel_jobs_use_disjoint_save_keys_and_a_common_restore_prefix() -> Result<(), RenderError> {
    let text = render_parallel_mbx()?;
    assert_eq!(
        text.matches("uses: ./.github/actions/rust-demo").count(),
        2,
        "both lanes must use the shared action:\n{text}"
    );
    Ok(())
}

#[test]
fn matrix_jobs_use_each_matrix_key_as_the_save_suffix() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            velnor_actions_workflow_renderer::checkout_step(&checkout_pin())?,
            velnor_actions_workflow_renderer::plan_step(),
        ],
    );
    let mut task = matrix_task_job()?.1;
    task.steps
        .extend(mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?);
    let text = render_workflow_ir(
        &fixture_ir(vec![plan, ("velnor-task".to_owned(), task)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(
        text.matches("MBX_MATRIX_KEY: ${{ matrix.matrix_key }}")
            .count(),
        1,
        "each matrix copy must pass its stable key to the bundle step:\n{text}"
    );
    assert!(
        text.contains("matrix: ${{ fromJSON(needs.plan.outputs.matrix) }}"),
        "{text}"
    );
    assert!(
        text.contains("key: ${{ steps.mbx-bundle-key.outputs.key }}"),
        "{text}"
    );
    assert!(text.contains("primary=\\\"${key}-${suffix}\\\""), "{text}");
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
