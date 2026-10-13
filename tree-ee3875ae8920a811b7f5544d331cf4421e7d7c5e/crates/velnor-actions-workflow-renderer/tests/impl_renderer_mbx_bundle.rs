//! Velnor's MBX bundle route archives one directory outside the live store.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let mbx = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    render_mbx_with_steps(id, scale_set, mbx)
}

fn render_mbx_with_steps(
    id: &str,
    scale_set: bool,
    mbx: [velnor_actions_contract::Step; 2],
) -> Result<String, RenderError> {
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

fn render_parallel_mbx()
-> Result<velnor_actions_workflow_renderer::render::RenderedWorkflow, RenderError> {
    let checkout = velnor_actions_workflow_renderer::checkout_step(&checkout_pin())?;
    let mbx = mbx_tool_steps(&mbx_uses(), "1.21.1", "1.98.1")?;
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout.clone(),
            acquire_fixture()?,
            velnor_actions_workflow_renderer::plan_step(),
        ],
    );
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
    render_workflow_ir_strict_shared(
        &fixture_ir(vec![plan, hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
}

fn assert_cold_import(imported: &str) {
    for needle in [
        "mbx cache import",
        "no mbx bundle matched",
        "matched-bundle-missing",
        "mbx bundle import failed; continuing cold",
        "df -B1 -P",
        "df -i -P",
        "acceptance=cache_unavailable",
        "cache_corrupt",
    ] {
        assert!(
            imported.contains(needle),
            "{needle} missing from {imported}"
        );
    }
    assert!(!imported.contains("test -d"), "{imported}");
    assert!(!imported.contains("rm -rf"), "{imported}");
}

#[test]
fn export_validates_owned_store_and_leaves_cleanup_to_runner_temp() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let init = text
        .find("name: Initialize private MBX store")
        .expect("private store init");
    let preflight = text
        .find("name: Verify MBX and Rust toolchains")
        .expect("toolchain preflight");
    let export = text
        .find("name: Export MBX single bundle")
        .expect("export step");
    let save = text
        .find("name: Save MBX single bundle")
        .expect("save step");
    let script = &text[export..save];
    let setup = &text[init..preflight];
    assert!(setup.contains("umask 077"), "{setup}");
    assert!(setup.contains("mktemp -d"), "{setup}");
    assert!(setup.contains("chmod 700"), "{setup}");
    assert!(setup.contains(".velnor-mbx-owner"), "{setup}");
    assert!(setup.contains("GITHUB_RUN_ID"), "{setup}");
    assert!(setup.contains("GITHUB_RUN_ATTEMPT"), "{setup}");
    assert!(setup.contains("GITHUB_JOB"), "{setup}");
    assert!(setup.contains("GITHUB_ENV"), "{setup}");
    assert!(
        !setup.contains("if: runner.environment"),
        "the v1.6 route owns both lanes: {setup}"
    );
    let bytes_before = script.find("df -B1 -P").expect("pre-export byte sample");
    let inodes_before = script.find("df -i -P").expect("pre-export inode sample");
    let gc = script.find("mbx gc").expect("reclaim");
    let exported = script.find("cache export").expect("export");
    let durable = script
        .find("[ ! -d \\\"$bundle\\\" ]")
        .expect("bundle check");
    let bytes_after = script[exported..]
        .find("df -B1 -P")
        .map(|at| at + exported)
        .expect("post-export byte sample");
    assert!(bytes_before < inodes_before, "{script}");
    assert!(inodes_before < gc, "{script}");
    assert!(gc < exported, "{script}");
    assert!(exported < durable, "{script}");
    assert!(durable < bytes_after, "{script}");
    assert!(script.contains("$MBX_CACHE_DIR/actions"), "{script}");
    assert!(script.contains("acceptance=cache_unavailable"), "{script}");
    assert!(script.contains("acceptance=accepted"), "{script}");
    assert!(script.contains("cleanup=runner-temp"), "{script}");
    assert!(
        !script.contains("rm -rf"),
        "store and partial bundle stay owned: {script}"
    );
    assert!(
        !script.contains("rmdir"),
        "store and partial bundle stay owned: {script}"
    );
    assert!(
        script.contains("$RUNNER_TEMP/mbx-single-bundle-export"),
        "{script}"
    );
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");
    assert!(!script.contains("github-actions-cache-v1"), "{script}");
    Ok(())
}

#[test]
fn hosted_bundle_key_and_restore_paths_are_disjoint() -> Result<(), RenderError> {
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
        text.find("name: Initialize private MBX store")
            .expect("private store init")
            < preflight
            && preflight < restore
            && restore < bundle_key
            && bundle_key < bundle
            && bundle < import
            && import < export
            && export < save,
        "{text}"
    );

    let key_step = &text[bundle_key..bundle];
    assert!(
        key_step.contains("MBX_JOB_ID: ${{ github.job }}"),
        "{key_step}"
    );
    assert!(key_step.contains("MBX_MATRIX_KEY: \"\""), "{key_step}");
    assert!(
        key_step.contains("primary=\\\"${writer_prefix}r${run_id}-a${run_attempt}\\\""),
        "{key_step}"
    );
    assert!(
        key_step.contains("prefix=\\\"$writer_prefix\\\""),
        "{key_step}"
    );
    assert!(
        key_step.contains("compatibility_key=\\\"${key%-*}\\\""),
        "{key_step}"
    );
    assert!(
        key_step.contains("fallback=\\\"${compatibility_key}-\\\""),
        "{key_step}"
    );
    assert!(
        key_step.contains("MBX_RUN_ID: ${{ github.run_id }}"),
        "{key_step}"
    );
    assert!(
        key_step.contains("MBX_RUN_ATTEMPT: ${{ github.run_attempt }}"),
        "{key_step}"
    );

    let restored = &text[bundle..import];
    assert!(restored.contains("actions/cache/restore@"), "{restored}");
    assert!(
        restored.contains("path: ${{ runner.temp }}/mbx-single-bundle-restore"),
        "{restored}"
    );
    assert!(
        !restored.contains("mbx-single-bundle-export"),
        "restore and export paths must stay disjoint: {restored}"
    );
    assert!(
        restored.contains(
            r#"restore-keys: "${{ steps.mbx-bundle-key.outputs.prefix }}\n${{ steps.mbx-bundle-key.outputs.fallback }}""#
        ),
        "stable writer prefix must precede common fallback: {restored}"
    );
    Ok(())
}

#[test]
fn hosted_bundle_import_continues_cold_on_cache_failure() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let import = text
        .find("name: Import MBX single bundle")
        .expect("bundle import");
    let export = text
        .find("name: Export MBX single bundle")
        .expect("export step");
    assert_cold_import(&text[import..export]);
    Ok(())
}

#[test]
fn hosted_action_and_save_use_one_bundle_owner() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let restore = text
        .find("name: Restore MBX objects")
        .expect("restore step");
    let bundle_key = text
        .find("name: Prepare MBX bundle key")
        .expect("bundle key");
    let action = &text[restore..bundle_key];
    assert!(action.contains("id: mbx"), "{action}");
    assert!(action.contains("ACTIONS_CACHE_MODE: read"), "{action}");
    assert!(!action.contains("write"), "{action}");

    let save = text
        .find("name: Save MBX single bundle")
        .expect("save step");
    let saved = &text[save..];
    assert!(
        saved.contains("path: ${{ runner.temp }}/mbx-single-bundle-export"),
        "{saved}"
    );
    assert!(
        !saved.contains("mbx-single-bundle-restore"),
        "save archives only the new export: {saved}"
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
    let rendered = render_parallel_mbx()?;
    assert_eq!(
        rendered
            .yaml
            .matches("uses: ./.github/actions/rust-demo")
            .count(),
        2,
        "both lanes must use the shared action:\n{}",
        rendered.yaml
    );
    let action = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/rust-demo/action.yml")
        .expect("generated shared action");
    let body = &action.bytes;
    for expression in [
        "MBX_JOB_ID: ${{ github.job }}",
        "MBX_RUN_ID: ${{ github.run_id }}",
        "MBX_RUN_ATTEMPT: ${{ github.run_attempt }}",
        "steps.mbx-bundle-key.outputs.prefix",
        "steps.mbx-bundle-key.outputs.fallback",
    ] {
        assert!(body.contains(expression), "missing {expression}: {body}");
    }
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
    assert!(text.contains("MBX_RUN_ID: ${{ github.run_id }}"), "{text}");
    assert!(
        text.contains("MBX_RUN_ATTEMPT: ${{ github.run_attempt }}"),
        "{text}"
    );
    assert!(
        text.contains("key: ${{ steps.mbx-bundle-key.outputs.key }}"),
        "{text}"
    );
    assert!(
        text.contains("primary=\\\"${writer_prefix}r${run_id}-a${run_attempt}\\\""),
        "{text}"
    );
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
