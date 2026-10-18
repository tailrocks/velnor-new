use std::error::Error;

use velnor_actions_contract::workflow::{
    TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
};
use velnor_actions_workflow_renderer::tree::RenderedTree;

/// Parse and validate the generated declared-task manifest.
pub(super) fn task_manifest(
    tree: &RenderedTree,
) -> Result<TaskExecutionManifestV1, Box<dyn Error>> {
    let marked = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or_else(|| std::io::Error::other("missing task execution manifest"))?;
    let (_, json) = marked
        .split_once('\n')
        .ok_or_else(|| std::io::Error::other("missing task execution marker"))?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(json)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Find one emitted task record by its discovered package and kind.
pub(super) fn task_record<'a>(
    prep: &'a velnor_actions_orchestrator::GenerationPreparation,
    manifest: &'a TaskExecutionManifestV1,
    package: &str,
    kind: velnor_actions_rust::TaskKind,
) -> Result<&'a TaskExecutionManifestEntryV1, Box<dyn Error>> {
    let proposal = prep
        .discovery
        .proposals
        .iter()
        .find(|task| {
            !task.no_targets && task.display_name == package && task.task_kind == kind.as_str()
        })
        .ok_or_else(|| {
            std::io::Error::other(format!("missing {package} {} task", kind.as_str()))
        })?;
    manifest.tasks.get(&proposal.task_id).ok_or_else(|| {
        std::io::Error::other(format!("task {} is not emitted", proposal.task_id)).into()
    })
}

/// Exact adjacent argv values in a manifest record.
pub(super) fn has_argv_pair(
    record: &TaskExecutionManifestEntryV1,
    first: &str,
    second: &str,
) -> bool {
    record
        .argv
        .windows(2)
        .any(|pair| pair[0] == first && pair[1] == second)
}

/// Check every discovered fuzz task stays scoped, featureless, and non-test-running.
pub(super) fn assert_fuzz_task_records(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    manifest: &TaskExecutionManifestV1,
    yaml: &str,
) -> Result<(), Box<dyn Error>> {
    for proposal in prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.display_name == "fuzz")
    {
        if proposal.no_targets {
            assert!(
                !manifest.tasks.contains_key(&proposal.task_id),
                "ineligible fuzz task is not emitted: {}",
                proposal.task_id
            );
            continue;
        }
        let record = manifest.tasks.get(&proposal.task_id).ok_or_else(|| {
            std::io::Error::other(format!("missing fuzz task {}", proposal.task_id))
        })?;
        if proposal.task_kind == "fmt" {
            assert!(has_argv_pair(
                record,
                "--manifest-path",
                "crates/fuzz/Cargo.toml"
            ));
        } else {
            assert!(has_argv_pair(record, "--package", "fuzz"));
        }
        assert!(
            !has_argv_pair(record, "nextest", "run")
                && !record.argv.iter().any(|arg| arg == "--doc"),
            "fuzz does not add Nextest or doctest runners: {:?}",
            record.argv
        );
        for arg in &record.argv {
            assert!(
                arg != "--no-default-features" && arg != "--features",
                "featureless fuzz keeps its defaults: {:?}",
                record.argv
            );
        }
        assert!(
            yaml.contains(&record.execution_digest),
            "workflow binds fuzz task {}",
            proposal.task_id
        );
    }
    Ok(())
}

/// Assert generated Nextest setup, execution, and testless-package behavior.
pub(super) fn assert_generated_nextest_commands(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    manifest: &TaskExecutionManifestV1,
    yaml: &str,
) -> Result<(), Box<dyn Error>> {
    let app_build = task_record(prep, manifest, "app", velnor_actions_rust::TaskKind::Build)?;
    assert!(
        has_argv_pair(app_build, "nextest", "list")
            && has_argv_pair(app_build, "--list-type", "binaries-only")
            && has_argv_pair(app_build, "--package", "app")
            && has_argv_pair(app_build, "--profile", "default"),
        "app prepares real Nextest test binaries under its selected profile: {:?}",
        app_build.argv
    );
    assert!(
        yaml.contains(&app_build.execution_digest),
        "workflow binds the app build record"
    );
    let app_nextest = task_record(
        prep,
        manifest,
        "app",
        velnor_actions_rust::TaskKind::Nextest,
    )?;
    assert!(
        has_argv_pair(app_nextest, "nextest", "run")
            && has_argv_pair(app_nextest, "--package", "app")
            && has_argv_pair(app_nextest, "--no-tests", "fail")
            && has_argv_pair(app_nextest, "--profile", "default"),
        "app keeps its Nextest run contract: {:?}",
        app_nextest.argv
    );
    assert!(
        yaml.contains(&app_nextest.execution_digest),
        "workflow binds the app Nextest record"
    );
    assert_fuzz_task_records(prep, manifest, yaml)?;
    Ok(())
}
