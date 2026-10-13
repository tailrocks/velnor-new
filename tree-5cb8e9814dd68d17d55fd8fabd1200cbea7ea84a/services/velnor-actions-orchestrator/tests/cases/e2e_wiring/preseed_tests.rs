//! Pre-seed helper ownership in the fully emitted workflow.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_cache::{
    cache_steps::MBX_RESTORE_NAME, cache_steps::MBX_VERSION_CHECK_NAME,
};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use super::{JobText, check_tree, check_verify_mbx, make_velnor_repo};
use crate::support::{TestResult, without_ambient_identity};

fn assert_native_preseed_owner(jobs: &[JobText], yaml: &str) -> TestResult {
    let builds: Vec<(&str, &str)> = jobs
        .iter()
        .flat_map(|job| {
            job.steps.iter().filter_map(|step| {
                if step.body.contains("mbx build --release --locked") {
                    Some((job.id.as_str(), step.body.as_str()))
                } else {
                    None
                }
            })
        })
        .collect();
    assert_eq!(builds.len(), 1, "exactly one helper build:\n{yaml}");
    assert_eq!(builds[0].0, "plan", "build lives in plan");
    for fragment in ["rust@", "--package velnor-actions-cli --bin velnor-actions"] {
        assert!(
            builds[0].1.contains(fragment),
            "build misses {fragment}:\n{yaml}"
        );
    }
    assert!(
        !builds[0].1.contains("mr-boxington@"),
        "the native action owns MBX installation:\n{yaml}"
    );
    let plan = jobs
        .iter()
        .find(|job| job.id == "plan")
        .ok_or("missing plan job")?;
    let position = |name: &str| plan.steps.iter().position(|step| step.name == name);
    let action_at = position(MBX_RESTORE_NAME).ok_or("missing native MBX action")?;
    let version_at = position(MBX_VERSION_CHECK_NAME).ok_or("missing MBX version guard")?;
    let build_at = plan
        .steps
        .iter()
        .position(|step| step.name.contains("Build helper"))
        .ok_or("missing pre-seed build")?;
    assert!(
        action_at < version_at && version_at < build_at,
        "native owner and version guard precede helper build:\n{yaml}"
    );
    assert!(
        plan.steps[action_at]
            .body
            .contains("uses: jdx/mr-boxington-action@"),
        "plan has the pinned native action:\n{yaml}"
    );
    assert!(
        plan.steps
            .iter()
            .any(|step| step.body.contains("name: velnor-preseed-helper")),
        "plan misses exact artifact upload:\n{yaml}"
    );
    let mbx = ToolCatalog::pinned()
        .version(PinnedTool::MrBoxington)
        .to_owned();
    check_verify_mbx(plan, &mbx)?;
    assert!(
        !yaml.contains("pattern:"),
        "no wildcard artifact matching:\n{yaml}"
    );
    Ok(())
}

fn assert_preseed_consumers(jobs: &[JobText], yaml: &str) {
    for job in jobs
        .iter()
        .filter(|job| job.id == "required" || job.id.starts_with("rust-"))
    {
        let has_download = job
            .steps
            .iter()
            .any(|step| step.name.contains("Download helper"));
        assert!(has_download, "{} misses download", job.id);
    }
    assert!(
        yaml.contains("pre-seed trust-on-review"),
        "trust marking missing:\n{yaml}"
    );
    assert!(
        !yaml.contains("Acquire Velnor"),
        "no digest path exists pre-seed:\n{yaml}"
    );
}

#[test]
fn emitted_yaml_preseed_builds_once_and_shares_artifact() -> TestResult {
    without_ambient_identity(
        "emitted_yaml_preseed_builds_once_and_shares_artifact",
        || {
            let repo = make_velnor_repo()?;
            assert!(
                !repo.path().join(".velnor/generator.lock").exists(),
                "pre-seed fixture must not carry a lock"
            );
            let prep = prepare(repo.path())?;
            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(WORKFLOW_PATH)
                .ok_or("missing workflow in staged tree")?;
            let jobs = check_tree(yaml).map_err(|err| format!("{err}:\n{yaml}"))?;
            assert_native_preseed_owner(&jobs, yaml)?;
            assert_preseed_consumers(&jobs, yaml);
            Ok(())
        },
    )
}

#[test]
fn empty_rust_discovery_prepares_local_helper_toolchain_before_mbx() -> TestResult {
    without_ambient_identity(
        "empty_rust_discovery_prepares_local_helper_toolchain_before_mbx",
        || {
            let repo = make_velnor_repo()?;
            std::fs::remove_file(repo.path().join("Cargo.toml"))?;
            std::fs::remove_dir_all(repo.path().join("src"))?;

            let prep = prepare(repo.path())?;
            assert!(prep.local_helper_build);
            assert!(prep.discovery.workspaces.is_empty());
            assert!(prep.discovery.statuses.iter().all(|status| match status {
                velnor_actions_contract_planning::DetectionStatus::Selected(project)
                | velnor_actions_contract_planning::DetectionStatus::Ignored { project, .. } => {
                    project.stack_id != "rust"
                }
            }));
            assert!(
                prep.discovery
                    .proposals
                    .iter()
                    .all(|task| task.stack_id != "rust")
            );

            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(WORKFLOW_PATH)
                .ok_or("missing workflow in staged tree")?;
            let jobs = super::parse_jobs(yaml);
            let plan = jobs
                .iter()
                .find(|job| job.id == "plan")
                .ok_or("missing Plan job")?;
            let position = |name: &str| plan.steps.iter().position(|step| step.name == name);
            let prepare_at = position("Prepare pinned tools").ok_or("missing tool setup")?;
            let rust_at = position("Verify Rust before MBX action").ok_or("missing Rust probe")?;
            let build_at = plan
                .steps
                .iter()
                .position(|step| step.name.contains("Build helper"))
                .ok_or("missing local helper build")?;
            let rust = ToolCatalog::pinned().tool_spec(PinnedTool::Rust);

            assert!(
                plan.steps[prepare_at].body.contains(&rust),
                "local helper build must install its compiler before the Rust and MBX probes:\n{yaml}"
            );
            assert!(
                prepare_at < rust_at && rust_at < build_at,
                "Rust installation precedes preflight and local helper build:\n{yaml}"
            );
            assert!(
                plan.steps[rust_at].body.contains("where")
                    && plan.steps[rust_at].body.contains("rust@1.98.1"),
                "the existing fail-closed Rust probe remains in place:\n{yaml}"
            );

            super::write_lock(&repo)?;
            let error = render_staged_tree(&prep)
                .err()
                .ok_or("new lock must invalidate the prepared local-build plan")?;
            assert!(
                error
                    .to_string()
                    .contains("generator_lock_presence_changed_after_prepare"),
                "lock appearance fails before helper setup can diverge: {error}"
            );
            Ok(())
        },
    )
}

#[test]
fn empty_rust_discovery_with_lock_keeps_release_acquisition_path() -> TestResult {
    without_ambient_identity(
        "empty_rust_discovery_with_lock_keeps_release_acquisition_path",
        || {
            let repo = make_velnor_repo()?;
            super::write_lock(&repo)?;
            std::fs::remove_file(repo.path().join("Cargo.toml"))?;
            std::fs::remove_dir_all(repo.path().join("src"))?;

            let prep = prepare(repo.path())?;
            assert!(!prep.local_helper_build);
            assert!(prep.discovery.workspaces.is_empty());
            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(WORKFLOW_PATH)
                .ok_or("missing workflow in staged tree")?;
            let jobs = super::parse_jobs(yaml);
            let plan = jobs
                .iter()
                .find(|job| job.id == "plan")
                .ok_or("missing Plan job")?;
            let prepare = plan
                .steps
                .iter()
                .find(|step| step.name == "Prepare pinned tools")
                .ok_or("missing tool setup")?;

            assert!(
                !prepare
                    .body
                    .contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust)),
                "lock-backed download needs no local Rust compiler:\n{yaml}"
            );
            assert!(
                !plan
                    .steps
                    .iter()
                    .any(|step| step.name == "Prepare Rust components"),
                "lock-backed no-Rust plan retains its previous setup sequence:\n{yaml}"
            );
            assert!(
                plan.steps.iter().any(|step| step.name == "Acquire Velnor"),
                "valid lock continues to use release acquisition:\n{yaml}"
            );
            assert!(
                plan.steps
                    .iter()
                    .all(|step| !step.name.contains("Build helper")),
                "valid lock must not build the helper locally:\n{yaml}"
            );

            std::fs::remove_file(repo.path().join(".velnor/generator.lock"))?;
            let error = render_staged_tree(&prep)
                .err()
                .ok_or("removed lock must invalidate the prepared plan")?;
            assert!(
                error
                    .to_string()
                    .contains("generator_lock_presence_changed_after_prepare"),
                "lock removal fails before a Rust-less local build can render: {error}"
            );
            Ok(())
        },
    )
}

#[test]
fn empty_rust_consumer_does_not_acquire_a_local_builder() -> TestResult {
    let repo =
        super::make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    std::fs::remove_file(repo.path().join("Cargo.toml"))?;
    std::fs::remove_dir_all(repo.path().join("src"))?;

    let prep = prepare(repo.path())?;
    assert!(!prep.local_helper_build);
    assert!(prep.discovery.workspaces.is_empty());
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    let jobs = super::parse_jobs(yaml);
    let plan = jobs
        .iter()
        .find(|job| job.id == "plan")
        .ok_or("missing Plan job")?;
    let prepare = plan
        .steps
        .iter()
        .find(|step| step.name == "Prepare pinned tools")
        .ok_or("missing tool setup")?;

    assert!(
        !prepare
            .body
            .contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust)),
        "ConsumerV1 must not acquire Rust for a Velnor-only helper path:\n{yaml}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components"),
        "ConsumerV1 no-Rust sequence remains unchanged:\n{yaml}"
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name.contains("Build helper")),
        "ConsumerV1 never uses the Velnor local helper build:\n{yaml}"
    );
    Ok(())
}
