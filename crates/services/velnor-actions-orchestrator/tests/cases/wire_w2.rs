//! FLOW-half wiring regression tests (W2).

use std::fs;
use std::path::Path;

use serde_json::json;
use velnor_actions_contract as C;
use velnor_actions_contract_planning as CD;
use velnor_actions_contract_workflow as CW;
use velnor_actions_orchestrator::{PlanOutputMode, plan_outputs, prepare, publish_plan_files};
use velnor_actions_orchestrator_core::decisions::plan_json_path;

use crate::cases::orch_core::{
    WireResult, has_warning, manifest_for, merge, merge_request, merge_request_for, plan_value,
    pr_value, success_jobs,
};
use crate::support::{
    TestResult, anchor_id, anchor_repo, config_with_branch, err_of, git, git_line, make_repo,
    passing_reports, plan_for, plan_for_source_change, write_nextest_task,
};

fn shard_config() -> String {
    format!(
        "{}\n[test_sharding]\ndefault_shards = 2\n",
        config_with_branch()
    )
}

#[test]
fn v1_push_plan_wiring() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert_eq!(plan.runner.label, "ubuntu-26.04");
    CD::validate_plan_edges(&plan.edges, &plan.task_ids)?;
    assert!(plan.edges.iter().any(|e| e.kind == CD::EdgeKind::Gate));
    for ob in &plan.obligations {
        assert_eq!(ob.decision, CW::ObligationDecision::Execute);
        assert_eq!(ob.reason, "affected_by_change");
        C::validate_digest(&ob.input_digest)?;
        C::validate_digest(&ob.task_digest)?;
    }
    for entry in &plan.matrix.include {
        let ids = entry.cache_ids.as_ref().expect("cache ids");
        C::validate_digest(ids.workspace_id())?;
        C::validate_digest(ids.lane_id())?;
        C::validate_digest(ids.platform_id())?;
        C::validate_digest(ids.toolchain_id())?;
        C::validate_digest(ids.cache_format_id())?;
        let meta = entry.adapter_metadata.as_object().expect("metadata");
        assert!(meta.contains_key("compile_driver") && meta.contains_key("test_runner"));
        assert!(meta.contains_key("evidence_ids"));
        assert_eq!(meta["task_cache_enabled"], false);
    }
    Ok(())
}

#[test]
fn input_digest_is_deterministic() -> TestResult {
    let (repo, first) = plan_for_source_change()?;
    let root = repo.path();
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "pull_request", Some(&base), &head, None)?;
    let second: CW::Plan = serde_json::from_value(value["plan"].clone())?;
    assert_eq!(first.obligations.len(), second.obligations.len());
    for (a, b) in first.obligations.iter().zip(&second.obligations) {
        assert_eq!((&a.task_id, &a.input_digest), (&b.task_id, &b.input_digest));
    }
    Ok(())
}

#[test]
fn build_script_package_rejects_reuse_and_coverage() -> TestResult {
    let dir = make_repo(config_with_branch())?;
    let root = dir.path();
    fs::write(root.join("build.rs"), "fn main() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let base = head.clone();
    let value = plan_value(root, "pull_request", Some(&base), &head, None)?;
    let plan: CW::Plan = serde_json::from_value(value["plan"].clone())?;
    assert!(!plan.obligations.is_empty());
    for ob in &plan.obligations {
        assert_eq!(ob.reason, "task_not_eligible", "{}", ob.task_id);
    }
    anchor_repo(root)?;
    let mut m = manifest_for(&plan, &base, "testmain")?;
    m["repository_id"] = json!(anchor_id());
    let warm = pr_value(root, &base, &head, Some(&m))?;
    let obs = warm["plan"]["obligations"].as_array().expect("obligations");
    assert!(obs.iter().all(|ob| ob["decision"] == "execute"));
    assert!(has_warning(&warm, "undeclared_inputs"));
    Ok(())
}

#[test]
fn reused_reports_fail_without_restore_proof() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    let first = reports.first_mut().expect("report");
    first.tasks[0].status = CW::TaskStatus::Reused;
    (first.reused, first.executed) = (1, 0);
    let final_report = merge(&merge_request_for(&plan, &reports)?)?;
    assert_eq!(final_report.status, CW::FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"task_result_incomplete".to_owned()),
        "reuse without proof fails: {:?}",
        final_report.miss_reasons
    );
    Ok(())
}

#[test]
fn nextest_shards_expand_plan_and_verify() -> TestResult {
    let dir = make_repo(&shard_config())?;
    let root = dir.path();
    write_nextest_task(root)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "push", None, &head, None)?;
    let plan: CW::Plan = serde_json::from_value(value["plan"].clone())?;
    let shards = plan.task_ids.iter().any(|id| id.contains("/shard-"));
    assert!(shards, "{:?}", plan.task_ids);
    let reports = passing_reports(&plan)?;
    let mut obs = plan.obligations.iter();
    let ob = obs
        .find(|ob| ob.task_id.contains("/shard-"))
        .expect("shard");
    let tests = json!([{"package": "demo", "target": "demo", "features": [], "binary": "demo", "name": "f"}]);
    let inventory = C::digest_b3(&C::canonical_json_bytes(&tests)?);
    let proof = json!({"task_id": ob.task_id, "input_digest": ob.input_digest, "runner": "cargo_nextest",
        "shard_index": 1, "shard_count": 1, "tests": tests, "inventory_digest": inventory, "no_test_targets": false});
    let mut request = merge_request_for(&plan, &reports)?;
    request["shard_proofs"] = json!([proof]);
    assert_eq!(merge(&request)?.status, CW::FinalStatus::Passed);
    Ok(())
}

#[test]
fn resource_groups_validate_at_merge() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let request_with = |groups: Vec<&str>| -> WireResult {
        let mut request = merge_request_for(&plan, &reports)?;
        request["limits"] = json!({"compiler_budget": 2, "test_budget": 4, "max_parallel": 2,
            "capacity": 4, "shards": 2, "retries": 0, "resource_groups": groups});
        Ok(request)
    };
    let bad = merge(&request_with(vec!["db", "cache"])?)?.status;
    assert_eq!(bad, CW::FinalStatus::PlanningFailed);
    let good = merge(&request_with(vec!["cache", "db"])?)?.status;
    assert_eq!(good, CW::FinalStatus::Passed);
    Ok(())
}

#[test]
fn manifest_task_proofs_validate_per_task() -> TestResult {
    let (repo, plan) = plan_for_source_change()?;
    let root = repo.path();
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    anchor_repo(root)?;
    let mut manifest = manifest_for(&plan, &head, "testmain")?;
    manifest["repository_id"] = json!(anchor_id());
    let tasks = manifest["tasks"].as_array_mut().expect("tasks");
    for (task, ob) in tasks.iter_mut().zip(&plan.obligations) {
        task["proof"] = json!({"task_id": ob.task_id, "task_digest": ob.task_digest, "input_digest": ob.input_digest,
            "graph_digest": ob.task_digest, "toolchain_id": ob.task_digest, "mbx_digest": ob.task_digest,
            "platform_id": ob.task_digest, "profile": "default", "proof_run_id": 7});
    }
    // Placeholder identity dimensions (graph/toolchain/platform set to
    // the task digest) no longer bind: live comparison refuses coverage
    // per task with a precise mismatch reason.
    let warm = plan_value(root, "pull_request", Some(&head), &head, Some(&manifest))?;
    for ob in warm["plan"]["obligations"].as_array().expect("obligations") {
        assert_eq!(ob["decision"], "execute", "{ob}");
    }
    assert!(has_warning(&warm, "proof_graph_mismatch"), "{warm}");
    // A proof that parses but no longer binds its entry fails per-task
    // re-validation with a precise mismatch reason.
    let mut bad = manifest_for(&plan, &head, "testmain")?;
    bad["repository_id"] = json!(anchor_id());
    let ob0 = &plan.obligations[0];
    bad["tasks"][0]["proof"] = json!({"task_id": ob0.task_id, "task_digest": ob0.task_digest,
        "input_digest": C::digest_b3(b"forged"), "graph_digest": ob0.task_digest,
        "toolchain_id": ob0.task_digest, "mbx_digest": ob0.task_digest,
        "platform_id": ob0.task_digest, "profile": "default", "proof_run_id": 7});
    let cold = plan_value(root, "pull_request", Some(&head), &head, Some(&bad))?;
    assert!(has_warning(&cold, "proof_mismatch"), "{cold}");
    // An unparseable proof never reaches per-task validation: the
    // boundary rejects the whole manifest as malformed.
    let mut malformed = manifest_for(&plan, &head, "testmain")?;
    malformed["repository_id"] = json!(anchor_id());
    malformed["tasks"][0]["proof"] = json!({"task_id": ob0.task_id, "task_digest": "bogus",
        "input_digest": ob0.input_digest, "graph_digest": ob0.task_digest,
        "toolchain_id": ob0.task_digest, "mbx_digest": ob0.task_digest,
        "platform_id": ob0.task_digest, "profile": "default", "proof_run_id": 7});
    let rejected = plan_value(root, "pull_request", Some(&head), &head, Some(&malformed))?;
    assert!(has_warning(&rejected, "malformed_manifest"), "{rejected}");
    Ok(())
}

#[test]
fn baseline_rejects_other_branch_manifests() -> TestResult {
    let (repo, plan) = plan_for_source_change()?;
    let root = repo.path();
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let m = manifest_for(&plan, &base, "other")?;
    let value = pr_value(root, &base, &head, Some(&m))?;
    assert!(has_warning(&value, "wrong_ref"));
    let m = manifest_for(&plan, &base, "testmain")?;
    let value = pr_value(root, &base, &head, Some(&m))?;
    assert!(has_warning(&value, "baseline_publish:forbidden"));
    Ok(())
}

#[test]
fn committed_drift_warns_never_fails() -> TestResult {
    let dir = make_repo(config_with_branch())?;
    let root = dir.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    let yml = root.join(".github/workflows/ci.yml");
    let body = "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate.\nrun: cargo nextest run --locked\nruns-on: windows-2025\n";
    fs::write(yml, body)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "push", None, &head, None)?;
    assert!(has_warning(&value, "committed_profile_drift"));
    assert!(has_warning(&value, "runner_family_changed"));
    assert!(
        !value["plan"]["task_ids"]
            .as_array()
            .expect("task ids")
            .is_empty()
    );
    let prep = prepare(root)?;
    assert!(prep.runner_image.is_unobserved());
    assert_eq!(prep.runner_image.image_os.as_str(), "unknown");
    assert_eq!(prep.runner_image.image_version.as_str(), "unknown");
    assert!(!prep.runner_label.is_empty());
    Ok(())
}

#[test]
fn local_select_uses_working_tree() -> TestResult {
    let dir = make_repo(config_with_branch())?;
    let root = dir.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\n",
    )?;
    for member in ["a", "b"] {
        let path = root.join(member);
        fs::create_dir_all(path.join("src"))?;
        let m =
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        fs::write(path.join("Cargo.toml"), m)?;
        fs::write(path.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    fs::write(root.join("b/src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    let value = plan_value(root, "local", None, &head, None)?;
    let ids = value["plan"]["task_ids"].as_array().expect("task ids");
    assert!(!ids.is_empty());
    for member in ["/a/", "/b/"] {
        assert!(
            ids.iter()
                .any(|id| id.as_str().is_some_and(|s| s.contains(member))),
            "{member} in universe: {ids:?}"
        );
    }
    let obs = value["plan"]["obligations"].as_array().expect("obs");
    assert!(
        obs.iter()
            .filter(|ob| ob["task_id"].as_str().is_some_and(|s| s.contains("/b/")))
            .all(|ob| ob["reason"] == "affected_by_change"),
        "{obs:?}"
    );
    assert!(
        obs.iter()
            .filter(|ob| ob["task_id"].as_str().is_some_and(|s| s.contains("/a/")))
            .all(|ob| ob["reason"] == "forced_uncached"),
        "{obs:?}"
    );
    git(&["checkout", "--", "."], root)?;
    let value = plan_value(root, "local", None, &head, None)?;
    assert!(
        !value["plan"]["task_ids"]
            .as_array()
            .expect("task ids")
            .is_empty()
    );
    assert!(has_warning(&value, "no_affected_files"));
    Ok(())
}

#[test]
fn fork_plans_use_pr_trust() -> TestResult {
    let (repo, _plan) = plan_for_source_change()?;
    let root = repo.path();
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "fork", Some(&base), &head, None)?;
    assert_eq!(value["plan"]["trust"], "pr");
    Ok(())
}

#[test]
fn matrix_reports_entries_per_task() -> TestResult {
    let dir = make_repo(config_with_branch())?;
    let prep = prepare(dir.path())?;
    let text = plan_for(&prep)?;
    assert!(text.contains("Entries:"), "{text}");
    for g in &prep.discovery.proposals {
        assert!(g.no_targets || text.contains(&g.task_id), "{text}");
    }
    Ok(())
}

#[test]
fn plan_files_match_contract_renderers() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let response = json!({"schema": 1, "plan": plan, "matrix": plan.matrix});
    let dir = tempfile::tempdir()?;
    let out = publish_plan_files(&response.to_string(), &dir.path().join("velnor"))?;
    assert_eq!(out, dir.path().join("velnor").join("local"));
    assert_eq!(
        fs::read(out.join("plan.json"))?,
        CW::plan_json_bytes(&plan)?
    );
    let matrix_bytes = CW::matrix_json_bytes(&plan.matrix)?;
    assert_eq!(fs::read(out.join("matrix.json"))?, matrix_bytes);
    let outputs = plan_outputs(&response.to_string(), PlanOutputMode::Static)?;
    assert_eq!(outputs.plan_id.as_str(), plan.plan_id.as_str());
    assert_eq!(outputs.run_key.as_str(), plan.run_key.as_str());
    let joined = plan_json_path(Path::new("/tmp/x/velnor"), "local")?;
    let want = std::path::PathBuf::from("/tmp/x/velnor/local/plan.json");
    assert_eq!(joined, want);
    Ok(())
}

#[test]
fn empty_matrix_folds_conclusions() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut plan_value = serde_json::to_value(&plan)?;
    plan_value["obligations"] = json!([]);
    plan_value["task_ids"] = json!([]);
    plan_value["matrix"]["include"] = json!([]);
    plan_value["edges"] = json!([]);
    let mut matrix = serde_json::to_value(&plan.matrix)?;
    matrix["include"] = json!([]);
    let request = merge_request(&plan_value, &matrix, &json!([]), &success_jobs());
    assert_eq!(merge(&request)?.status, CW::FinalStatus::NoWork);
    let failed = json!([{"job_id": "plan", "conclusion": "failure"}]);
    let request = merge_request(&plan_value, &matrix, &json!([]), &failed);
    assert_eq!(merge(&request)?.status, CW::FinalStatus::Failed);
    Ok(())
}

#[test]
fn cargo_test_shards_reject_config() -> TestResult {
    let dir = make_repo(&shard_config())?;
    let err = err_of(prepare(dir.path()), "cargo_test shards")?;
    assert!(
        err.to_string().contains("cargo_test_single_obligation"),
        "{err}"
    );
    Ok(())
}

#[test]
fn fmt_needs_explicit_config() -> TestResult {
    let dir = make_repo(config_with_branch())?;
    let prep = prepare(dir.path())?;
    let groups = &prep.discovery.proposals;
    assert!(!groups.iter().any(|g| g.task_id.contains("/fmt/")));
    fs::write(dir.path().join("rustfmt.toml"), "")?;
    let prep = prepare(dir.path())?;
    let groups = &prep.discovery.proposals;
    assert!(groups.iter().any(|g| g.task_id.contains("/fmt/")));
    Ok(())
}
