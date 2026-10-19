//! Plan-selection acceptance: broaden and shards (OW2).

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::{
    commit, has, inv_digest, make_ws, merge_status, plan_at, plan_change, proof, put, reasons_are,
    shard_plan, sharded_reports, test_value,
};
use serde_json::Value as Json;
use velnor_actions_contract::FinalStatus;
use velnor_actions_mise::catalog::RUST_VERSION;

#[test]
fn generator_and_workflow_changes_broaden() -> TestResult {
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[(
            ".velnor/config.toml",
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n# tuned\n",
        )],
    )?;
    assert!(
        has(&plan, "alpha") && has(&plan, "beta"),
        "config broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("selecting_all")),
        "{warnings:?}"
    );
    let (plan, _) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[(".github/workflows/ci.yml", "name: ci\n")],
    )?;
    assert!(
        has(&plan, "alpha") && has(&plan, "beta"),
        "workflow broadens: {:?}",
        plan.task_ids
    );
    Ok(())
}

/// Every crate job is selected by a change to a declared tool version.
fn selects_everything(extra: &[(&str, &str)], change: &[(&str, &str)], label: &str) -> TestResult {
    let (plan, warnings) = plan_change(&["alpha", "beta"], &[], extra, &[], change)?;
    assert!(
        reasons_are(&plan, "alpha", "affected_by_change")
            && reasons_are(&plan, "beta", "affected_by_change"),
        "{label} selects everything: {:?}",
        plan.obligations
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("project_tool_config_changed:all_changed")),
        "{label}: {warnings:?}"
    );
    Ok(())
}

/// A change to a project tool source that leaves every declared catalog
/// version alone selects no crate job.
fn selects_nothing(extra: &[(&str, &str)], change: &[(&str, &str)], label: &str) -> TestResult {
    let (plan, warnings) = plan_change(&["alpha", "beta"], &[], extra, &[], change)?;
    assert!(
        !reasons_are(&plan, "alpha", "affected_by_change")
            && !reasons_are(&plan, "beta", "affected_by_change"),
        "{label} must not select everything: {:?}",
        plan.obligations
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("project_tool_config_changed")),
        "{label}: {warnings:?}"
    );
    Ok(())
}

#[test]
fn project_tool_version_edits_select_everything() -> TestResult {
    // Hard decision 5: CI runs the versions these files declare, so a
    // changed declaration re-tests every crate.
    let rust_toolchain = format!("[toolchain]\nchannel = \"{RUST_VERSION}\"\n");
    let declared = format!("[tools]\nrust = \"{RUST_VERSION}\"\nnode = \"20.1.0\"\n");
    selects_everything(&[], &[("mise.toml", declared.as_str())], "new declaration")?;
    selects_everything(
        &[],
        &[("rust-toolchain.toml", rust_toolchain.as_str())],
        "toolchain file",
    )?;
    selects_everything(
        &[("mise.toml", "[tools]\nrelease-plz = \"0.3.1\"\n")],
        &[("mise.toml", "[tools]\nrelease-plz = \"0.3.2\"\n")],
        "bumped version",
    )?;
    selects_everything(
        &[("mise.toml", declared.as_str())],
        &[("mise.toml", "[tools]\nnode = \"20.1.0\"\n")],
        "removed declaration",
    )?;
    selects_everything(
        &[],
        &[(".config/mise/conf.d/ci.toml", declared.as_str())],
        "conf.d",
    )?;
    selects_everything(
        &[],
        &[(".tool-versions", "release-plz 0.3.2\n")],
        ".tool-versions",
    )?;
    selects_everything(&[], &[("mise.lock", "# lock\n")], "lock file")
}

#[test]
fn unrelated_project_tool_source_edits_select_nothing() -> TestResult {
    // The same files edited without touching a declared catalog version:
    // a task body, a non-catalog pin, a comment.
    let before = format!(
        "[tools]\nrust = \"{RUST_VERSION}\"\nnode = \"20.1.0\"\n\n[tasks.a]\nrun = \"echo a\"\n"
    );
    let after = format!(
        "# edited\n[tools]\nrust = \"{RUST_VERSION}\"\nnode = \"22.0.0\"\n\n[tasks.a]\nrun = \"echo b\"\n"
    );
    selects_nothing(
        &[("mise.toml", before.as_str())],
        &[("mise.toml", after.as_str())],
        "task and node edit",
    )?;
    selects_nothing(
        &[(".tool-versions", "release-plz 0.3.1\njava 21\n")],
        &[(".tool-versions", "release-plz 0.3.1 # pinned\njava 22\n")],
        ".tool-versions java edit",
    )?;
    // A new source that declares no catalog tool.
    selects_nothing(
        &[],
        &[("mise.local.toml", "[tools]\nnode = \"22.0.0\"\n")],
        "new local file",
    )
}

#[test]
fn untracked_files_broaden() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let head = commit(repo.path(), "one")?;
    put(repo.path(), "notes.md", "draft\n")?;
    let (plan, warnings) = plan_at(repo.path(), Some(&head), &head)?;
    assert!(
        has(&plan, "alpha") && has(&plan, "beta"),
        "broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("untracked_files")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn cache_miss_never_fails_task() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let entry = plan.matrix.include.first().ok_or("missing entry")?;
    let ob = plan.obligations.first().ok_or("missing ob")?;
    let id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &ob.task_digest,
    )?;
    let report: velnor_actions_contract::TaskReport = serde_json::from_value(
        serde_json::json!({"schema": velnor_actions_contract::TaskReport::SCHEMA, "task_report_id": id, "run_key": "local", "event": "pull_request", "trust": "pr", "matrix_id": entry.id, "matrix_key": entry.matrix_key, "task_id": ob.task_id, "task_digest": ob.task_digest, "status": "executed", "cache": {"layer": "task", "key": "k", "result": "miss", "miss_reason": "no_entry"}, "platform_binding": {"state": "unavailable", "planned_platform_id": entry.planned_platform.platform_id, "runner_environment": "unknown", "reason": "observation_not_recorded"}, "exit_code": 0, "duration_ms": 1, "outputs": []}),
    )?;
    report.validate()?;
    let reports = passing_reports(&plan)?;
    assert_eq!(
        merge_status(&plan, &reports, &[], &Json::Object(serde_json::Map::new()))?,
        FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn shard_aggregation_rejects_mismatch_extra_and_unproven() -> TestResult {
    let (plan, first, second, input) = shard_plan()?;
    let mut both = vec![test_value("one"), test_value("two")];
    let inv = inv_digest(&mut both)?;
    let good = vec![
        proof(&first, &input, 1, &[test_value("one")], &inv),
        proof(&second, &input, 2, &[test_value("two")], &inv),
    ];
    let reports = sharded_reports(&plan, &[first.clone(), second.clone()])?;
    let empty = Json::Object(serde_json::Map::new());
    let mut mismatch = good.clone();
    mismatch[1]["archive_digest"] = Json::String(velnor_actions_contract::digest_b3(b"other"));
    assert_eq!(
        merge_status(&plan, &reports, &mismatch, &empty)?,
        FinalStatus::PlanningFailed
    );
    let mut extra = good.clone();
    extra[1]["tests"] = Json::Array(vec![test_value("two"), test_value("three")]);
    assert_eq!(
        merge_status(&plan, &reports, &extra, &empty)?,
        FinalStatus::PlanningFailed
    );
    let empty_inv = inv_digest(&mut [])?;
    let unproven = vec![
        proof(&first, &input, 1, &[], &empty_inv),
        proof(&second, &input, 2, &[], &empty_inv),
    ];
    assert_eq!(
        merge_status(&plan, &reports, &unproven, &empty)?,
        FinalStatus::PlanningFailed
    );
    let base_lim = serde_json::json!({"compiler_budget": 4, "test_budget": 4, "max_parallel": 4, "capacity": 8, "shards": 2, "retries": 0});
    for (key, value) in [
        ("compiler_budget", 0),
        ("test_budget", 0),
        ("max_parallel", 0),
        ("max_parallel", 9),
    ] {
        let mut limits = base_lim.clone();
        limits[key] = Json::from(value);
        assert_eq!(
            merge_status(
                &plan,
                &reports,
                &good,
                &serde_json::json!({"limits": limits})
            )?,
            FinalStatus::PlanningFailed,
            "{key}={value}"
        );
    }
    assert_eq!(
        merge_status(&plan, &reports, &good, &empty)?,
        FinalStatus::Passed
    );
    Ok(())
}
