//! End-to-end schema-2 named-check placement and plan/report identity.

use serde_json::{Value, json};

use crate::impl_common::{TestResult, git};
use crate::impl_schema2_routing::{
    execution_head, job_body, profiles, release_repo, required_file,
};
use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;
use velnor_actions_orchestrator_internal::internal::plan_internal;

fn check_config(id: &str, task: &str, label: &str, platform: &str) -> String {
    format!(
        "[[checks]]\nid = \"{id}\"\ntask = \"{task}\"\ndirectory = \".\"\ninputs = [\"mise.toml\"]\ntools = []\ntimeout_minutes = 20\n[checks.runner]\nlabel = \"{label}\"\nplatform = \"{platform}\"\nexecutor = \"hosted\"\n"
    )
}

fn config(mode: &str, overrides: &str) -> String {
    format!(
        "{}\nmode = \"{mode}\"\n{}\n{overrides}\n{}{}",
        execution_head(),
        profiles(),
        check_config("linux", "check:linux", "ubuntu-26.04", "linux_x64"),
        check_config("mac", "check:mac", "macos-15", "macos_arm64")
    )
}

fn fixture(config: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = release_repo(config)?;
    let root = repo.path();
    std::fs::write(
        root.join("mise.toml"),
        "[tasks.\"check:linux\"]\nrun = 'true'\n[tasks.\"check:mac\"]\nrun = 'true'\n",
    )?;
    git(&["add", "mise.toml"], root)?;
    git(&["commit", "-m", "add named check tasks"], root)?;
    Ok(repo)
}

#[test]
fn generated_hosted_scale_set_and_both_modes_bind_all_named_check_proofs() -> TestResult {
    for (mode, linux_ids, linux_runner) in [
        ("hosted", ["check-linux", ""], "runs-on: ubuntu-26.04"),
        (
            "scale-set",
            ["check-linux", ""],
            "runs-on: [velnor, ubuntu-26.04-scale-set]",
        ),
        (
            "both",
            ["check-linux__hosted", "check-linux__local"],
            "runs-on: ubuntu-26.04",
        ),
    ] {
        assert_mode(mode, &linux_ids, linux_runner)?;
    }
    Ok(())
}

fn assert_mode(mode: &str, linux_ids: &[&str], linux_runner: &str) -> TestResult {
    let repo = fixture(&config(mode, ""))?;
    let root = repo.path();
    let prep = prepare(root)?;
    let tree = render_staged_tree(&prep)?;
    assert_workflow(&tree, mode, linux_ids, linux_runner)?;
    assert_plan_lanes(root, mode)
}

fn assert_workflow(
    tree: &velnor_actions_workflow_tree::RenderedTree,
    mode: &str,
    linux_ids: &[&str],
    linux_runner: &str,
) -> TestResult {
    let ci = required_file(tree, ".github/workflows/ci.yml")?;
    let required = job_body(ci, "required")?;
    for id in linux_ids.iter().copied().filter(|id| !id.is_empty()) {
        assert!(
            required.contains(id),
            "{mode} Required omits {id}: {required}"
        );
        assert!(
            ci.contains(&format!("  {id}:")),
            "{mode} omits job {id}: {ci}"
        );
    }
    assert!(
        required.contains("check-mac"),
        "{mode} Required omits mac check"
    );
    let mac = job_body(ci, "check-mac")?;
    assert!(
        mac.contains("runs-on: macos-15"),
        "{mode} moved macOS: {mac}"
    );
    assert!(
        !mac.contains("date +%s%3N"),
        "native checks use monotonic Rust timing"
    );
    if mode == "both" {
        let hosted = job_body(ci, "check-linux__hosted")?;
        let scale = job_body(ci, "check-linux__local")?;
        assert!(hosted.contains(linux_runner), "{hosted}");
        assert!(scale.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
        assert!(hosted.contains("VELNOR_CHECK_LANE_VARIANT: hosted"));
        assert!(scale.contains("VELNOR_CHECK_LANE_VARIANT: scale_set"));
    } else {
        let linux = job_body(ci, "check-linux")?;
        assert!(linux.contains(linux_runner), "{mode}: {linux}");
        assert!(!ci.contains("check-linux__hosted"));
        assert!(!ci.contains("check-linux__local"));
    }
    Ok(())
}

fn assert_plan_lanes(root: &std::path::Path, mode: &str) -> TestResult {
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "event": "local",
        "base": null,
        "head": "local",
        "root": root.display().to_string(),
    });
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let entries = response["plan"]["matrix"]["include"]
        .as_array()
        .ok_or("plan matrix missing")?;
    let linux = entries
        .iter()
        .filter(|entry| {
            entry["job_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("check-linux"))
        })
        .collect::<Vec<_>>();
    let mac = entries
        .iter()
        .filter(|entry| entry["job_id"] == "check-mac")
        .collect::<Vec<_>>();
    if mode == "both" {
        assert_eq!(linux.len(), 2, "{mode}: {linux:?}");
        assert_eq!(linux[0]["lane_variant"], "hosted");
        assert_eq!(linux[1]["lane_variant"], "scale_set");
        for key in ["id", "matrix_key", "report_id", "artifact_id", "job_id"] {
            assert_ne!(linux[0][key], linux[1][key], "lane {key} must be unique");
        }
    } else {
        assert_eq!(linux.len(), 1, "{mode}: {linux:?}");
        assert!(linux[0].get("lane_variant").is_none());
        assert_eq!(linux[0]["job_id"], "check-linux");
    }
    assert_eq!(mac.len(), 1, "macOS remains one required lane in {mode}");
    assert_eq!(mac[0]["job_id"], "check-mac");
    let linux_task = linux[0]["task_id"]
        .as_str()
        .ok_or("named check task id missing")?;
    assert_eq!(
        linux
            .iter()
            .filter(|entry| entry["task_id"].as_str() == Some(linux_task))
            .count(),
        linux.len()
    );
    assert_eq!(
        response["plan"]["obligations"]
            .as_array()
            .ok_or("obligations missing")?
            .iter()
            .filter(|obligation| obligation["task_id"].as_str() == Some(linux_task))
            .count(),
        1,
        "paired lanes retain one logical obligation"
    );
    Ok(())
}

#[test]
fn macos_check_scale_set_override_is_rejected_explicitly() -> TestResult {
    let overrides =
        "[execution.overrides.\"check-mac\"]\nprofile = \"local\"\nrole = \"verification\"\n";
    let repo = fixture(&config("both", overrides))?;
    let err =
        render_staged_tree(&prepare(repo.path())?).expect_err("macOS cannot use Linux scale set");
    assert!(
        err.to_string()
            .contains("verification_runner_incompatible_with_scale_set"),
        "{err}"
    );
    Ok(())
}
