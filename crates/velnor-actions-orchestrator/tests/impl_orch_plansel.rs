//! Plan-selection acceptance: narrow selection (OW2).
use crate::impl_common::{
    TestResult, config_with_branch, fixture_manifest_json, git, git_line, passing_reports,
    plan_for_source_change,
};
use crate::impl_merge::task_reports_for;
use serde_json::Value as Json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use velnor_actions_contract::{FinalStatus, MatrixReport, Plan};
use velnor_actions_orchestrator::{baseline_artifact_numeric_id, merge_internal, plan_internal};

/// Git repo with workspace `members` plus path `deps` as (from, to, table).
pub(crate) fn make_ws(
    members: &[&str],
    deps: &[(&str, &str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "t@e.c"], root)?;
    git(&["config", "user.name", "T"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    let list = members
        .iter()
        .map(|m| format!("\"{m}\""))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers=[{list}]\n"),
    )?;
    for member in members {
        fs::create_dir_all(root.join(member).join("src"))?;
        let mut manifest =
            format!("[package]\nname=\"{member}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n");
        for dep in deps.iter().filter(|dep| dep.0 == *member) {
            use std::fmt::Write as _;
            write!(
                manifest,
                "\n[{}]\n{}={{path=\"../{}\"}}\n",
                dep.2, dep.1, dep.1
            )?;
        }
        fs::write(root.join(member).join("Cargo.toml"), manifest)?;
        fs::write(root.join(member).join("src/lib.rs"), "pub fn f(){}\n")?;
    }
    Ok(dir)
}

/// Commit everything; returns the new HEAD SHA.
pub(crate) fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", message], root)?;
    git_line(&["rev-parse", "HEAD"], root)
}

/// Write `path` under `root`, creating parent dirs.
pub(crate) fn put(root: &Path, path: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(root.join(parent))?;
    }
    fs::write(root.join(path), text)?;
    Ok(())
}

/// Appended source line marking a changed file.
pub(crate) const BUMP: &str = "pub fn f(){}\npub fn g(){}\n";

/// Plan a PR `base..head` on a fresh workspace after applying a change.
pub(crate) fn plan_change(
    members: &[&str],
    deps: &[(&str, &str, &str)],
    extra: &[(&str, &str)],
    remove: &[&str],
    change: &[(&str, &str)],
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let repo = make_ws(members, deps)?;
    for (path, text) in extra {
        put(repo.path(), path, text)?;
    }
    let base = commit(repo.path(), "one")?;
    for path in remove {
        fs::remove_file(repo.path().join(path))?;
    }
    for (path, text) in change {
        put(repo.path(), path, text)?;
    }
    let head = commit(repo.path(), "two")?;
    plan_at(repo.path(), Some(&base), &head, None)
}

/// Plan PR `base..head` with an optional manifest; plan + warnings.
pub(crate) fn plan_at(
    root: &Path,
    base: Option<&str>,
    head: &str,
    manifest: Option<Json>,
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let mut request = serde_json::json!({"schema": 1, "run_key": "local", "base": base, "head": head, "event": "pull_request", "root": root.display().to_string()});
    if let Some(manifest) = manifest {
        request["baseline_manifest"] = manifest;
    }
    let response = plan_internal(&request.to_string())?;
    let value: Json = serde_json::from_str(&response)?;
    Ok((
        serde_json::from_value(value["plan"].clone())?,
        serde_json::from_value(value["plan"]["warnings"].clone())?,
    ))
}

/// True when some selected task mentions `needle`.
pub(crate) fn has(plan: &Plan, needle: &str) -> bool {
    plan.task_ids.iter().any(|id| id.contains(needle))
}

/// True when a member's obligations exist and all carry `reason`.
pub(crate) fn reasons_are(plan: &Plan, needle: &str, reason: &str) -> bool {
    let mut any = false;
    for ob in plan
        .obligations
        .iter()
        .filter(|ob| ob.task_id.contains(needle))
    {
        any = true;
        if ob.reason != reason {
            return false;
        }
    }
    any
}

pub(crate) use crate::impl_common::{anchor_id, anchor_repo};

/// Manifest binding `base` with `tasks` entries for the seed plan.
pub(crate) fn manifest_for(plan: &Plan, base: &str, tasks: &Json) -> Json {
    let compat = velnor_actions_contract::digest_b3(b"compat");
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let name = format!("velnor-baseline-{base}-{compat}");
    let numeric = baseline_artifact_numeric_id(&name);
    serde_json::json!({"schema": 2, "repository_id": anchor_id(), "source_commit": base, "ref": "refs/heads/testmain", "event": "push", "workflow_ref": format!("o/r/{workflow}@refs/heads/testmain"), "run_id": 7, "run_attempt": 1, "final_status": "passed", "generator_version": plan.generator.version, "generator_sha256": plan.generator.sha256, "compatibility_id": compat, "artifact_id": numeric, "artifact_name": name, "tasks": tasks})
}

/// Task entries binding every seed obligation exactly.
pub(crate) fn entries_for(plan: &Plan) -> Json {
    Json::Array(plan.obligations.iter().map(|ob| serde_json::json!({"task_id": ob.task_id, "task_digest": ob.task_digest, "input_digest": ob.input_digest, "closure_digest": ob.closure_digest, "proof_run_id": 7, "observed_run_id": 7})).collect())
}

/// One test identity JSON value.
pub(crate) fn test_value(name: &str) -> Json {
    serde_json::json!({"package": "pkg", "target": "lib", "features": ["default"], "binary": "pkg-test", "name": name})
}

/// Inventory digest over canonical sorted test values.
pub(crate) fn inv_digest(tests: &mut [Json]) -> Result<String, Box<dyn std::error::Error>> {
    tests.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(velnor_actions_contract::digest_b3(
        &velnor_actions_contract::canonical_json_bytes(&tests)?,
    ))
}

/// One shard proof JSON for `task` with partition `tests`.
pub(crate) fn proof(task: &str, input: &str, index: u32, tests: &[Json], inv: &str) -> Json {
    serde_json::json!({"task_id": task, "input_digest": input, "runner": "cargo_nextest", "shard_index": index, "shard_count": 2, "tests": tests, "inventory_digest": inv, "archive_digest": velnor_actions_contract::digest_b3(b"archive"), "no_test_targets": false})
}

/// Sharded plan plus shard IDs and their shared input digest.
pub(crate) fn shard_plan() -> Result<(Plan, String, String, String), Box<dyn std::error::Error>> {
    let (_repo, mut plan) = plan_for_source_change()?;
    let base = plan.obligations.remove(0);
    let ids: Vec<String> = [1, 2]
        .iter()
        .map(|i| format!("{}/shard-{i}-of-2", base.task_id))
        .collect();
    for id in &ids {
        let mut ob = base.clone();
        ob.task_id.clone_from(id);
        ob.task_digest =
            velnor_actions_contract::digest_b3(format!("{}{id}", base.task_digest).as_bytes());
        plan.obligations.push(ob);
    }
    plan.obligations.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    plan.task_ids = plan
        .obligations
        .iter()
        .map(|ob| ob.task_id.clone())
        .collect();
    plan.task_ids.sort();
    plan.edges
        .retain(|edge| edge.from != base.task_id && edge.to != base.task_id);
    let entry = plan.matrix.include.first_mut().ok_or("missing entry")?;
    entry.execute_task_ids.tasks = std::collections::BTreeMap::from([(
        "nextest".to_owned(),
        velnor_actions_contract::ExecuteTaskRef::Shards(ids.clone()),
    )]);
    plan.validate()?;
    let input = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == ids[0])
        .ok_or("missing ob")?
        .input_digest
        .clone();
    Ok((plan, ids[0].clone(), ids[1].clone(), input))
}

/// Executed reports for the sharded pair plus passing reports elsewhere.
pub(crate) fn sharded_reports(
    plan: &Plan,
    pair: &[String],
) -> Result<Vec<MatrixReport>, Box<dyn std::error::Error>> {
    let mut trimmed = plan.clone();
    trimmed.matrix.include.remove(0);
    let mut reports = passing_reports(&trimmed)?;
    let entry = plan.matrix.include.first().ok_or("missing entry")?;
    let mut tasks = Vec::new();
    for id in pair {
        let ob = plan
            .obligations
            .iter()
            .find(|ob| &ob.task_id == id)
            .ok_or("missing ob")?;
        let rid = velnor_actions_contract::task_report_id_for_task(
            "local",
            &entry.matrix_key,
            &ob.task_digest,
        )?;
        tasks.push(serde_json::json!({"task_report_id": rid, "task_id": id, "status": "executed", "exit_code": 0}));
    }
    tasks.sort_by(|a, b| a["task_id"].as_str().cmp(&b["task_id"].as_str()));
    let mut rids: Vec<Json> = tasks.iter().map(|t| t["task_report_id"].clone()).collect();
    rids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    let mut ids: Vec<Json> = pair.iter().map(|id| Json::String(id.clone())).collect();
    ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    let report: MatrixReport = serde_json::from_value(
        serde_json::json!({"schema": 1, "report_id": entry.report_id, "run_key": "local", "matrix_id": entry.id, "matrix_key": entry.matrix_key, "status": "passed", "expected_task_ids": ids, "task_report_ids": rids, "tasks": tasks, "selected": 2, "reused": 0, "executed": 2, "empty_partition": 0, "not_selected": 0, "failed": 0, "cancelled": 0}),
    )?;
    report.validate()?;
    reports.insert(0, report);
    Ok(reports)
}

/// Merge status for a sharded plan, reports, proofs, and extras.
pub(crate) fn merge_status(
    plan: &Plan,
    reports: &[MatrixReport],
    proofs: &[Json],
    extra: &Json,
) -> Result<FinalStatus, Box<dyn std::error::Error>> {
    let plan_value = serde_json::to_value(plan).unwrap_or(Json::Null);
    let task_files = task_reports_for(
        &plan_value,
        &serde_json::to_value(reports).unwrap_or(Json::Null),
    );
    let mut request = serde_json::json!({"schema": 1, "run_key": "local", "actual_event": plan_value.get("event").cloned().unwrap_or(Json::Null), "plan": plan, "matrix": plan.matrix, "matrix_reports": reports, "task_reports": task_files, "required_job_ids": ["plan"], "required_jobs": [{"job_id": "plan", "conclusion": "success"}], "shard_proofs": proofs});
    for (key, value) in extra.as_object().ok_or("not an object")? {
        request[key] = value.clone();
    }
    Ok(
        serde_json::from_str::<velnor_actions_contract::FinalReport>(&merge_internal(
            &request.to_string(),
        )?)?
        .status,
    )
}

#[test]
fn rename_within_package_marks_owner_affected() -> TestResult {
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &["beta/src/lib.rs"],
        &[("beta/src/main.rs", "pub fn f(){}\n")],
    )?;
    assert!(has(&plan, "alpha") && has(&plan, "beta"), "universe kept");
    assert!(
        reasons_are(&plan, "beta", "affected_by_change"),
        "owner affected: {:?}",
        plan.obligations
    );
    assert!(
        reasons_are(&plan, "alpha", "forced_uncached"),
        "peer forced_uncached: {:?}",
        plan.obligations
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("all_changed") || w.contains("comparison_unavailable")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn diamond_closure_marks_transitively() -> TestResult {
    let deps = [
        ("b", "a", "dependencies"),
        ("c", "a", "dependencies"),
        ("d", "b", "dependencies"),
        ("d", "c", "dev-dependencies"),
    ];
    let (plan, _) = plan_change(
        &["a", "b", "c", "d"],
        &deps,
        &[],
        &[],
        &[("a/src/lib.rs", BUMP)],
    )?;
    for member in ["/a/", "/b/", "/c/", "/d/"] {
        assert!(
            reasons_are(&plan, member, "affected_by_change"),
            "{member} affected: {:?}",
            plan.obligations
        );
    }
    let (plan, warnings) = plan_change(
        &["a", "b", "c", "d"],
        &deps,
        &[],
        &[],
        &[("d/src/lib.rs", BUMP)],
    )?;
    assert!(
        reasons_are(&plan, "/d/", "affected_by_change"),
        "leaf affected: {:?} {warnings:?}",
        plan.obligations
    );
    for member in ["/a/", "/b/", "/c/"] {
        assert!(
            reasons_are(&plan, member, "forced_uncached"),
            "{member} forced_uncached: {:?} {warnings:?}",
            plan.obligations
        );
    }
    let (plan, warnings) = plan_change(
        &["a", "b", "c", "d"],
        &deps,
        &[],
        &[],
        &[("c/src/lib.rs", BUMP)],
    )?;
    assert!(
        reasons_are(&plan, "/c/", "affected_by_change")
            && reasons_are(&plan, "/d/", "affected_by_change"),
        "dev edge propagates: {:?} {warnings:?}",
        plan.obligations
    );
    assert!(
        reasons_are(&plan, "/a/", "forced_uncached")
            && reasons_are(&plan, "/b/", "forced_uncached"),
        "others forced_uncached: {:?}",
        plan.obligations
    );
    Ok(())
}

#[test]
fn build_script_and_fixture_changes_mark_owner() -> TestResult {
    let deps = [("alpha", "beta", "dependencies")];
    let (plan, _) = plan_change(
        &["alpha", "beta"],
        &deps,
        &[("beta/build.rs", "fn main(){}\n")],
        &[],
        &[("beta/build.rs", "fn main(){println!(\"hi\");}\n")],
    )?;
    assert!(
        reasons_are(&plan, "alpha", "affected_by_change")
            && reasons_are(&plan, "beta", "affected_by_change"),
        "closure: {:?}",
        plan.obligations
    );
    let (plan, _) = plan_change(
        &["alpha", "beta"],
        &deps,
        &[("alpha/tests/data.json", "{}\n")],
        &[],
        &[("alpha/tests/data.json", "{\"v\":1}\n")],
    )?;
    assert!(
        reasons_are(&plan, "alpha", "affected_by_change"),
        "alpha affected: {:?}",
        plan.obligations
    );
    assert!(
        reasons_are(&plan, "beta", "forced_uncached"),
        "beta forced_uncached: {:?}",
        plan.obligations
    );
    Ok(())
}
