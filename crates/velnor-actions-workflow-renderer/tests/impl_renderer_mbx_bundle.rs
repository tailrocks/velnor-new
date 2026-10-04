//! Hosted MBX uses one explicit directory bundle and a private store per job.

use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_workflow_renderer::steps::{MBX_PREFLIGHT_NAME, mbx_objects_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

fn mbx_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

pub(super) fn mbx_job(id: &str, version: &str) -> Result<(String, Job), RenderError> {
    let mbx = mbx_objects_step(&mbx_uses(), false, version)?;
    let pinned_tools = Step {
        name: "Prepare pinned tools".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "install".to_owned(),
                "rust@1.98.1".to_owned(),
                format!("mr-boxington@{version}"),
            ],
            env: BTreeMap::from([
                (
                    "MISE_CARGO_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/cargo".to_owned(),
                ),
                (
                    "MISE_RUSTUP_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/rustup".to_owned(),
                ),
                ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
            ]),
        },
    };
    Ok(job(id, "MBX job", Vec::new(), vec![pinned_tools, mbx]))
}

fn render_mbx(id: &str, scale_set: bool) -> Result<String, RenderError> {
    let mut built = mbx_job(id, TEST_MBX_VERSION)?;
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
fn private_root_precedes_local_setup_and_external_restore() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    assert_preflight_order(&text);
    assert_preflight_content(&text);
    assert_private_root_content(&text);
    assert_local_setup_content(&text);
    assert_mbx_key_content(&text);
    Ok(())
}

fn assert_preflight_order(text: &str) {
    let prepare = step_index(text, "Prepare pinned tools");
    let preflight = step_index(text, MBX_PREFLIGHT_NAME);
    let root = step_index(text, "Prepare private MBX store");
    let setup = step_index(text, "Setup MBX");
    let key = step_index(text, "Prepare MBX bundle key");
    let restore = step_index(text, "Restore MBX single bundle");
    let import = step_index(text, "Import MBX single bundle");
    let export = step_index(text, "Export MBX single bundle");
    let save = step_index(text, "Save MBX single bundle");
    assert_eq!(
        text.matches(&format!("name: {MBX_PREFLIGHT_NAME}")).count(),
        1
    );
    assert!(prepare < preflight && preflight < root && root < setup);
    assert!(setup < key && key < restore && restore < import);
    assert!(import < export && export < save);
}

fn assert_preflight_content(text: &str) {
    let preflight = step_index(text, MBX_PREFLIGHT_NAME);
    let root = step_index(text, "Prepare private MBX store");
    let preflight_script = &text[preflight..root];
    assert!(
        preflight_script.contains("mr-boxington@1.22.0"),
        "preflight resolves the exact installed MBX: {preflight_script}"
    );
    assert!(
        preflight_script.contains("rust@1.98.1") && preflight_script.contains("GITHUB_PATH"),
        "preflight verifies and publishes the exact Rustup shim: {preflight_script}"
    );
}

fn assert_private_root_content(text: &str) {
    let root = step_index(text, "Prepare private MBX store");
    let setup = step_index(text, "Setup MBX");
    let root_script = &text[root..setup];
    assert!(root_script.contains("mktemp -d"), "{root_script}");
    assert!(
        root_script.contains("GITHUB_OUTPUT.mbx-root"),
        "{root_script}"
    );
    assert!(root_script.contains("MBX_CACHE_DIR=%s"), "{root_script}");
    assert!(
        root_script.contains("MBX_TARGET_ROOT=%s/targets"),
        "{root_script}"
    );
    assert!(
        root_script.contains("MBX_SHIMS_DIR=%s/shims"),
        "{root_script}"
    );
    assert!(
        root_script.contains("MBX_CACHE_EXPORT_GROUP=%s"),
        "{root_script}"
    );
    assert!(root_script.contains("stable MBX bundle path already exists"));
    assert!(root_script.contains("velnor-mbx-store.XXXXXXXXXX"));
    assert!(!root_script.contains("rm -"), "{root_script}");
}

fn assert_local_setup_content(text: &str) {
    let setup = step_index(text, "Setup MBX");
    let key = step_index(text, "Prepare MBX bundle key");
    let local = &text[setup..key];
    assert!(local.contains("id: mbx"), "{local}");
    assert!(local.contains("backend: local"), "{local}");
    assert!(local.contains("version: 1.22.0"), "{local}");
    for forbidden in [
        "github-cache-mode",
        "cache-generation",
        "ACTIONS_CACHE_MODE",
    ] {
        assert!(
            !local.contains(forbidden),
            "implicit cache behavior: {local}"
        );
    }
}

fn assert_mbx_key_content(text: &str) {
    let key = step_index(text, "Prepare MBX bundle key");
    let restore = step_index(text, "Restore MBX single bundle");
    let key_yaml = &text[key..restore];
    for component in [
        "MBX_GENERATION: velnor-mbx-1.22.0",
        "MBX_EXPECTED_VERSION: 1.22.0",
        "github.event.pull_request.base.sha",
        "MBX_CACHE_SCOPE: demo",
        "toJSON(matrix)",
        "GITHUB_WORKFLOW_REF",
        "mise --no-config --no-env --no-hooks exec",
        "rust@$RUSTUP_TOOLCHAIN",
        "set -o pipefail",
        "$GITHUB_OUTPUT.mbx-scope",
        "$GITHUB_OUTPUT.mbx-rustc",
        "sha256sum",
        "scope-${scope_hash}-",
    ] {
        assert!(key_yaml.contains(component), "{component}: {key_yaml}");
    }
    assert!(!key_yaml.contains("toolchain=norust"), "{key_yaml}");
    assert!(key_yaml.contains("mise --no-config --no-env --no-hooks exec"));
    assert!(key_yaml.contains("-- rustc -vV"));
}

#[test]
fn mbx_yaml_id_uses_action_identity_not_display_name() -> Result<(), RenderError> {
    let (id, mut job) = mbx_job("demo", "1.21.1")?;
    let action_at = job
        .steps
        .iter()
        .position(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("jdx/mr-boxington-action@")))
        .expect("MBX action");
    job.steps[action_at].name = "Install MBX runtime".to_owned();
    job.steps.insert(
        action_at,
        scrubbed_shell_step("Setup MBX", vec!["true".to_owned()])?,
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![(id, job)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let setup = text.find("name: Setup MBX").expect("shell label");
    let action = text
        .find("name: Install MBX runtime")
        .expect("action label");
    let bundle_key = text
        .find("name: Prepare MBX bundle key")
        .expect("bundle key");
    assert!(!text[setup..action].contains("id: mbx"));
    assert!(text[action..bundle_key].contains("id: mbx"));
    Ok(())
}

#[test]
fn restore_import_and_save_share_stable_path_and_trusted_key() -> Result<(), RenderError> {
    let text = render_mbx("demo", false)?;
    let restore = step_index(&text, "Restore MBX single bundle");
    let import = step_index(&text, "Import MBX single bundle");
    let export = step_index(&text, "Export MBX single bundle");
    let save = step_index(&text, "Save MBX single bundle");
    assert_bundle_restore(&text, restore, import);
    assert_import_fallback(&text, import, export);
    assert_export_gc(&text, export, save);
    assert_bundle_save(&text, save);
    assert!(
        !text.contains("rm -rf"),
        "no recursive deletion is generated"
    );
    Ok(())
}

fn step_index(text: &str, name: &str) -> usize {
    required_index(text.find(&format!("name: {name}")), name)
}

fn required_index(index: Option<usize>, description: &str) -> usize {
    assert!(index.is_some(), "missing expected text: {description}");
    index.unwrap_or_default()
}

fn assert_bundle_restore(text: &str, restore: usize, import: usize) {
    let restored = &text[restore..import];
    assert!(restored.contains("actions/cache/restore@"), "{restored}");
    assert!(
        restored.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{restored}"
    );
    assert!(
        restored.contains("key: ${{ steps.mbx-bundle-key.outputs.primary }}"),
        "{restored}"
    );
    assert!(
        restored.contains("restore-keys: ${{ steps.mbx-bundle-key.outputs.prefix }}"),
        "{restored}"
    );
}

fn assert_import_fallback(text: &str, import: usize, export: usize) {
    let imported = &text[import..export];
    for needle in [
        "mbx cache import",
        "no MBX bundle matched",
        "matched MBX bundle is missing",
        "import failed; abandoning its private store",
        "GITHUB_OUTPUT.mbx-fallback",
        "velnor-mbx-fallback.XXXXXXXXXX",
        "MBX_CACHE_DIR=%s",
        "MBX_TARGET_ROOT=%s/targets",
        "MBX_SHIMS_DIR=%s/shims",
        "fresh cold MBX store selected",
        "GITHUB_ENV",
    ] {
        assert!(
            imported.contains(needle),
            "{needle} missing from {imported}"
        );
    }
    assert!(!imported.contains("rm -"), "{imported}");
    assert!(!imported.contains("continue-on-error"), "{imported}");
}

fn assert_export_gc(text: &str, export: usize, save: usize) {
    let script = &text[export..save];
    let before = required_index(script.find("df -B1 -P"), "initial byte sample");
    let export_at = required_index(script.find("mbx cache export"), "export");
    let durable = required_index(script.find("test -d"), "export destination check");
    let gc = required_index(script.find("mbx gc --max-size 0 --json"), "official GC");
    let after = required_index(script.rfind("df -i -P"), "final inode sample");
    assert!(before < export_at && export_at < durable && durable < gc && gc < after);
    assert!(
        script.contains("gc-succeeded=false"),
        "GC failure is recorded: {script}"
    );
    let gc_failed = required_index(script.find("gc-succeeded=false"), "GC failure output");
    assert!(
        script[gc_failed..].contains("ready=false"),
        "GC failure suppresses save: {script}"
    );
    assert!(script.contains("ready=true"), "{script}");
    assert!(!script.contains("rm -"), "{script}");
    assert_eq!(script.matches("--format directory").count(), 1, "{script}");
    assert!(
        script.contains("$RUNNER_TEMP/mbx-single-bundle"),
        "{script}"
    );
}

fn assert_bundle_save(text: &str, save: usize) {
    let saved = &text[save..];
    assert!(
        saved.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{saved}"
    );
    assert!(
        saved.contains("key: ${{ steps.mbx-bundle-key.outputs.primary }}"),
        "{saved}"
    );
    assert!(
        saved.contains("github.ref_name == github.event.repository.default_branch"),
        "default-branch-only trust: {saved}"
    );
    assert!(saved.contains("steps.mbx-export.outputs.ready == 'true'"));
    assert!(!saved.contains("pull_request"), "{saved}");
}

#[test]
fn mbx_store_environment_cannot_be_overridden_by_a_step() -> Result<(), RenderError> {
    for key in [
        "MBX_CACHE_DIR",
        "MBX_TARGET_ROOT",
        "MBX_SHIMS_DIR",
        "MBX_CACHE_EXPORT_GROUP",
    ] {
        let mut built = mbx_job("demo", "1.21.1")?;
        let Some(step) = built.1.steps.iter_mut().find(|step| match &step.kind {
            StepKind::Action { uses, .. } => uses.contains("mr-boxington-action@"),
            StepKind::Shell { .. } | StepKind::Internal { .. } => false,
        }) else {
            return Err(RenderError::InvalidWorkflow("missing_mbx_step".to_owned()));
        };
        let StepKind::Action { env, .. } = &mut step.kind else {
            return Err(RenderError::InvalidWorkflow("bad_mbx_step".to_owned()));
        };
        env.insert(key.to_owned(), "/shared/store".to_owned());
        let result = render_workflow_ir(
            &fixture_ir(vec![built]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        );
        assert!(
            result.is_err_and(|error| format!("{error:?}").contains("mbx_private_env_override")),
            "{key} override must fail closed"
        );
    }
    Ok(())
}

#[test]
fn scale_set_jobs_keep_private_root_and_local_backend() -> Result<(), RenderError> {
    let text = render_mbx("rust-demo__local", true)?;
    assert!(text.contains("name: Prepare private MBX store"), "{text}");
    assert!(text.contains("backend: local"), "{text}");
    assert!(!text.contains("ACTIONS_CACHE_MODE"), "{text}");
    assert!(!text.contains("MBX_GC_AUTO"), "{text}");
    Ok(())
}
