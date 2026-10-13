//! Helpers for the rendered task-contract preview test.

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::{TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator::GenerationPreparation;
use velnor_actions_workflow_renderer::tree::RenderedTree;

pub(super) fn task_manifest(
    tree: &RenderedTree,
) -> Result<TaskExecutionManifestV1, Box<dyn std::error::Error>> {
    let marked = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or("missing task execution manifest")?;
    let (_, json) = marked
        .split_once('\n')
        .ok_or("missing task execution marker")?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(json)?;
    manifest.validate()?;
    Ok(manifest)
}

pub(super) fn window<'a>(
    yaml: &'a str,
    from: &str,
    to: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    let start = yaml.find(from).ok_or("missing window start")?;
    let end = yaml[start..].find(to).map_or(yaml.len(), |at| start + at);
    Ok(&yaml[start..end])
}

pub(super) fn assert_crate_contract(
    prep: &GenerationPreparation,
    manifest: &TaskExecutionManifestV1,
    yaml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow_env = window(yaml, "env:\n", "concurrency:")?;
    let task = window(yaml, "  rust-demo:", "  required:")?;
    let crate_job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or("missing rust-demo IR job")?;
    let clippy_step = crate_job
        .steps
        .iter()
        .find(|step| step.name == "Clippy")
        .ok_or("missing Clippy task")?;
    let StepKind::TaskExecution {
        task_id,
        env: task_env,
        ..
    } = &clippy_step.kind
    else {
        return Err("Clippy must be a declared task".into());
    };
    let clippy_record = manifest
        .tasks
        .get(task_id)
        .ok_or("missing Clippy task manifest record")?;
    let rustup_toolchain = ToolCatalog::pinned().rustup_toolchain();
    assert_eq!(clippy_record.env, *task_env, "manifest copies Clippy env");
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME"] {
        assert!(
            clippy_record
                .env
                .get(key)
                .is_some_and(|value| !value.is_empty()),
            "Clippy manifest misses {key}"
        );
    }
    assert!(
        clippy_record
            .env
            .get("RUSTUP_TOOLCHAIN")
            .is_some_and(|value| value == &rustup_toolchain),
        "Clippy manifest carries the catalog Rust toolchain"
    );
    assert!(
        task.contains(&clippy_record.execution_digest),
        "workflow binds Clippy to its validated manifest record"
    );
    for key in [
        "MISE_NO_CONFIG:",
        "MISE_NO_ENV:",
        "MISE_NO_HOOKS:",
        "MISE_LOCKFILE:",
        "MISE_AUTO_INSTALL:",
        "MISE_EXEC_AUTO_INSTALL:",
    ] {
        assert!(workflow_env.contains(key), "workflow misses {key}");
    }
    assert!(
        task.contains(&format!("RUSTUP_TOOLCHAIN: {rustup_toolchain}")),
        "crate job selects the catalog Rust toolchain"
    );
    assert_credentials_scrubbed(workflow_env);
    Ok(())
}

fn assert_credentials_scrubbed(workflow_env: &str) {
    for key in [
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(
            workflow_env.contains(&format!("{key}: \"\"")),
            "workflow must scrub {key} empty"
        );
    }
}
