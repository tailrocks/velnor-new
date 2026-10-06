//! T32 E2E negatives over a consumer-shaped root-`.` repo.
//!
//! Malformed `.tf` fails plan closed with a recorded cause; a failed
//! plan reds Required. Valid-but-unformatted `.tf` rides plan green,
//! then its failed fmt leg (exit 3, the real `tofu fmt` signal)
//! reds Required with the cause recorded. Module and lock changes
//! select the calling root `.`. Hermetic: `TempDir` git fixtures,
//! inline bytes, constructed lane reports — no tofu binary spawn.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{FinalStatus, MatrixStatus, Plan, TaskStatus};
use velnor_actions_orchestrator::{
    OrchestratorError, merge_internal, merge_passed, prepare, publish_final_report,
};

use super::impl_common::{TestResult, git, install_fixture_release_manifest, passing_reports};
use super::impl_orch_core::{merge, merge_request, set_task, success_jobs};
use super::impl_select::{commit, plan_pr};

/// Consumer-shaped config: single tofu root at `.`.
fn root_dot_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\".\"]\n"
        .to_owned()
}

/// Provider-backed root calling one local child module, lock committed.
fn consumer_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "main.tf",
            "module \"policy\" {\n  source = \"./modules/repository-policy\"\n}\nresource \"example\" \"r\" {}\n",
        ),
        (
            "modules/repository-policy/main.tf",
            "variable \"policy\" {}\n",
        ),
        (
            ".terraform.lock.hcl",
            "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n",
        ),
    ]
}

/// Git-initialized pure-tofu repo at root `.`: `config` plus `files`.
fn make_root_dot_repo(
    config: &str,
    files: &[(&str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    install_fixture_release_manifest(root)?;
    for (relative, content) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    Ok(dir)
}

/// Plan base..head after rewriting `changed` to `content` in a commit.
fn plan_after_edit(
    changed: &str,
    content: &str,
) -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let dir = make_root_dot_repo(&root_dot_config(), &consumer_files())?;
    let root = dir.path();
    let base = commit(root, "base")?;
    fs::write(root.join(changed), content)?;
    let head = commit(root, "head")?;
    let (plan, _) = plan_pr(root, Some(&base), &head)?;
    plan.validate()?;
    Ok((dir, plan))
}

/// Task IDs the root-`.` triple must carry (preview-pinned shape).
fn root_triple() -> [&'static str; 3] {
    [
        "stack/tofu/dir-/fmt/default",
        "stack/tofu/dir-/init/default",
        "stack/tofu/dir-/validate/default",
    ]
}

/// Malformed `.tf` fails plan closed naming the file with a diagnostic.
#[test]
fn t32_malformed_tf_fails_plan_closed_with_cause() -> TestResult {
    let dir = make_root_dot_repo(&root_dot_config(), &consumer_files())?;
    let root = dir.path();
    let base = commit(root, "base")?;
    // Unclosed block: same shape as fixtures/tofu-malformed/hcl-syntax.
    fs::write(root.join("main.tf"), "variable \"x\" {\n  default = 1\n")?;
    let head = commit(root, "head")?;
    let before = fs::read(root.join("main.tf"))?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let err = velnor_actions_orchestrator::plan_internal(&request.to_string())
        .err()
        .ok_or_else(|| std::io::Error::other("malformed plan planned"))?;
    assert!(
        matches!(err, OrchestratorError::Detection { .. }),
        "detection error, got {err:?}"
    );
    let text = err.to_string();
    assert!(
        text.contains("malformed_manifest:main.tf"),
        "names the file: {text}"
    );
    assert!(!text.is_empty(), "diagnostic rides along: {text}");
    assert!(
        prepare(root).is_err(),
        "prepare fails closed on the same head"
    );
    assert_eq!(fs::read(root.join("main.tf"))?, before, "read-only");
    Ok(())
}

/// A failed plan validator reds Required with its conclusion recorded.
#[test]
fn t32_failed_plan_reds_required_with_recorded_cause() -> TestResult {
    let (_repo, plan) = plan_after_edit("main.tf", "variable \"x\" {}\n")?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let jobs = json!([{"job_id": "plan", "conclusion": "failure"}]);
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &jobs,
    );
    let report = merge(&request)?;
    report.validate()?;
    assert_eq!(report.status, FinalStatus::Failed);
    assert!(
        report
            .required_job_results
            .iter()
            .any(|job| job.job_id == "plan"
                && job.conclusion == velnor_actions_contract::JobConclusion::Failure),
        "plan failure recorded: {:?}",
        report.required_job_results
    );
    Ok(())
}

/// A failed fmt leg (exit 3) reds Required; the published verdict stays red.
#[test]
fn t32_unformatted_tf_failed_fmt_leg_reds_required() -> TestResult {
    // Valid HCL no formatter would keep: plan must still select the root.
    let (_repo, plan) = plan_after_edit("main.tf", "variable    \"x\"    {}\n")?;
    let fmt = root_triple()[0];
    assert!(
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == fmt && ob.reason == "affected_by_change"),
        "fmt obligation affected: {:?}",
        plan.obligations
            .iter()
            .map(|ob| (&ob.task_id, &ob.reason))
            .collect::<Vec<_>>()
    );
    let mut reports = passing_reports(&plan)?;
    let index = plan
        .matrix
        .include
        .iter()
        .position(|entry| entry.task_id == fmt)
        .ok_or_else(|| std::io::Error::other("fmt leg missing"))?;
    set_task(
        &mut reports[index],
        TaskStatus::Failed,
        MatrixStatus::Failed,
    )?;
    reports[index].tasks[0].exit_code = 3;
    reports[index].validate()?;
    let plan_value = serde_json::to_value(&plan)?;
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let response = merge_internal(&request.to_string())?;
    assert!(!merge_passed(&response)?, "failed fmt never passes");
    let final_report: velnor_actions_contract::FinalReport = serde_json::from_str(&response)?;
    assert_eq!(final_report.status, FinalStatus::Failed);
    assert_eq!(final_report.counts.failed, 1, "one failed task counted");
    assert_eq!(reports[index].tasks[0].exit_code, 3, "cause keeps exit 3");
    let dir = tempfile::TempDir::new()?;
    let artifact = publish_final_report(&response, dir.path())?;
    let published = fs::read_to_string(artifact.join("final-report.json"))?;
    assert!(!merge_passed(&published)?, "published verdict stays red");
    Ok(())
}

/// A module change selects exactly the calling root `.` triple.
#[test]
fn t32_module_change_selects_root_dot() -> TestResult {
    let (_repo, plan) = plan_after_edit(
        "modules/repository-policy/main.tf",
        "variable \"policy\" {}\nvariable \"extra\" {}\n",
    )?;
    assert_eq!(plan.matrix.include.len(), 3, "root triple planned");
    for task in root_triple() {
        assert!(
            plan.obligations
                .iter()
                .any(|ob| ob.task_id == task && ob.reason == "affected_by_change"),
            "{task} affected"
        );
    }
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.reason == "affected_by_change"),
        "only the calling triple: {:?}",
        plan.obligations
            .iter()
            .map(|ob| (&ob.task_id, &ob.reason))
            .collect::<Vec<_>>()
    );
    Ok(())
}

/// A lock change selects exactly the root `.` triple.
#[test]
fn t32_lock_change_selects_root_dot() -> TestResult {
    let (_repo, plan) = plan_after_edit(
        ".terraform.lock.hcl",
        "provider \"example.com/a/b\" {\nversion = \"1.0.1\"\n}\n",
    )?;
    assert_eq!(plan.matrix.include.len(), 3, "root triple planned");
    for task in root_triple() {
        assert!(
            plan.obligations
                .iter()
                .any(|ob| ob.task_id == task && ob.reason == "affected_by_change"),
            "{task} affected"
        );
    }
    Ok(())
}

/// Positive control: a clean `.tf` change rides plan to a green merge.
#[test]
fn t32_clean_tf_change_merges_passed() -> TestResult {
    let (_repo, plan) = plan_after_edit("main.tf", "variable \"x\" {}\n")?;
    assert_eq!(plan.matrix.include.len(), 3, "root triple planned");
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}
