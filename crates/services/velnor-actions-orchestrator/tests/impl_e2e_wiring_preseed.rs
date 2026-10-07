//! Pre-seed helper ownership in the fully emitted workflow.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_cache::{
    cache_steps::MBX_RESTORE_NAME, cache_steps::MBX_VERSION_CHECK_NAME,
};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use super::{JobText, check_tree, check_verify_mbx, make_velnor_repo};
use crate::impl_common::{TestResult, without_ambient_identity};

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
