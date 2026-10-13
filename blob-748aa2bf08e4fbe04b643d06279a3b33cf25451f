//! Assertions for declared task data emitted by W1 workflow tests.

use std::error::Error;

use velnor_actions_contract::workflow::{TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1};
use velnor_actions_workflow_renderer::tree::RenderedTree;

/// Check a Format wrapper call against the pinned toolchain in its manifest.
pub(crate) fn assert_format_task_record(
    task_id: &str,
    job: &str,
    tree: &RenderedTree,
) -> Result<(), Box<dyn Error>> {
    let marked_manifest = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or("missing task execution manifest")?;
    let (_, manifest_json) = marked_manifest
        .split_once('\n')
        .ok_or("task execution manifest marker")?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(manifest_json)?;
    manifest.validate()?;
    let record = manifest.tasks.get(task_id).ok_or("missing Format record")?;
    assert_eq!(record.task_id, task_id);
    assert!(
        job.contains(&record.execution_digest),
        "Format action must bind its manifest record: {}\n{job}",
        record.execution_digest
    );
    for (key, expected) in [
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.99.0"),
    ] {
        assert_eq!(
            record.env.get(key).map(String::as_str),
            Some(expected),
            "Format manifest record must keep the owned toolchain value for {key}"
        );
    }
    Ok(())
}
