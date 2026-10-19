//! Plan-selection acceptance: broaden and shards (OW2).

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::{
    BUMP, commit, has, inv_digest, make_ws, merge_status, plan_at, plan_change, proof, put,
    reasons_are, shard_plan, sharded_reports, test_value,
};
use serde_json::Value as Json;
use velnor_actions_contract::{FinalStatus, ObligationDecision};
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

#[test]
fn toolfile_edits_leave_universe_unproven() -> TestResult {
    let rust_toolchain = format!("[toolchain]\nchannel = \"{RUST_VERSION}\"\n");
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[
            ("mise.toml", "[tools]\n"),
            ("rust-toolchain.toml", rust_toolchain.as_str()),
        ],
    )?;
    assert!(
        has(&plan, "alpha") && has(&plan, "beta"),
        "universe kept: {:?}",
        plan.task_ids
    );
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::NotAffected && ob.reason == "not_affected"),
        "all not_affected: {:?}",
        plan.obligations
    );
    assert!(
        warnings.iter().any(|w| w.contains("toolfiles_only")),
        "{warnings:?}"
    );
    let (plan, _) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[("mise.toml", "[tools]\n"), ("beta/src/lib.rs", BUMP)],
    )?;
    assert!(has(&plan, "alpha") && has(&plan, "beta"), "universe kept");
    assert!(
        reasons_are(&plan, "beta", "affected_by_change"),
        "beta affected: {:?}",
        plan.obligations
    );
    assert!(
        reasons_are(&plan, "alpha", "not_affected"),
        "alpha not_affected: {:?}",
        plan.obligations
    );
    Ok(())
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
