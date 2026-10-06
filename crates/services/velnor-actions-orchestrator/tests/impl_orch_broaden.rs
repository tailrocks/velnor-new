//! Plan-selection acceptance: broaden, baseline, shards (OW2).

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::{
    BUMP, anchor_repo, commit, entries_for, has, inv_digest, make_ws, manifest_for, merge_status,
    plan_at, plan_change, proof, put, reasons_are, shard_plan, sharded_reports, test_value,
};
use serde_json::Value as Json;
use velnor_actions_contract_workflow::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::plan_internal;

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
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[
            ("mise.toml", "[tools]\n"),
            ("rust-toolchain.toml", "[toolchain]\n"),
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
            .all(|ob| ob.decision == ObligationDecision::Execute && ob.reason == "forced_uncached"),
        "all forced_uncached: {:?}",
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
        reasons_are(&plan, "alpha", "forced_uncached"),
        "alpha forced_uncached: {:?}",
        plan.obligations
    );
    Ok(())
}

#[test]
fn untracked_files_broaden() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let head = commit(repo.path(), "one")?;
    put(repo.path(), "notes.md", "draft\n")?;
    let (plan, warnings) = plan_at(repo.path(), Some(&head), &head, None)?;
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
fn exact_baseline_covers_unchanged() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    anchor_repo(repo.path())?;
    let head = seed.head.clone();
    let (bare, _) = plan_at(repo.path(), Some(&head), &head, None)?;
    assert!(!bare.task_ids.is_empty(), "empty diff keeps universe");
    assert!(
        bare.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute && ob.reason == "forced_uncached"),
        "empty diff forced_uncached without baseline: {:?}",
        bare.obligations
    );
    let manifest = manifest_for(&bare, &head, &entries_for(&bare));
    let (plan, _) = plan_at(repo.path(), Some(&head), &head, Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
    );
    assert!(plan.matrix.include.is_empty(), "matrix pruned");
    assert_eq!(plan.task_ids, bare.task_ids, "plan retains all");
    assert_eq!(
        plan.baseline.status(),
        velnor_actions_contract_workflow::BaselineStatus::Used
    );
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.baseline_proof.is_some())
    );
    Ok(())
}

#[test]
fn baseline_miss_reasons_are_precise() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let base = seed.base.clone().ok_or("missing base")?;
    let tasks = entries_for(&seed);
    for (field, value, reason) in [
        ("ref", "refs/heads/other", "wrong_ref"),
        ("generator_version", "0.0.0", "generator_mismatch"),
    ] {
        let mut manifest = manifest_for(&seed, &base, &tasks);
        manifest[field] = Json::String(value.into());
        let (plan, warnings) = plan_at(repo.path(), Some(&base), &seed.head, Some(manifest))?;
        assert!(
            plan.obligations
                .iter()
                .all(|ob| ob.decision == ObligationDecision::Execute),
            "{reason}"
        );
        assert!(
            warnings.iter().any(|w| w.contains(reason)),
            "{reason}: {warnings:?}"
        );
    }
    Ok(())
}

#[test]
fn carried_proof_fails_closed_without_originating_attestation() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    anchor_repo(repo.path())?;
    let head = seed.head.clone();
    let (bare, _) = plan_at(repo.path(), Some(&head), &head, None)?;
    let mut tasks = entries_for(&bare);
    for task in tasks.as_array_mut().ok_or("tasks shape")? {
        task["proof_run_id"] = Json::from(5);
        task["observed_run_id"] = Json::from(7);
    }
    let manifest = manifest_for(&bare, &head, &tasks);
    let (plan, warnings) = plan_at(repo.path(), Some(&head), &head, Some(manifest))?;
    // A forwarded proof run claims success for an originating run the
    // manifest cannot prove: validation fails closed with its precise
    // reason instead of warning-and-covering.
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert_eq!(
        plan.baseline.reason(),
        Some("baseline_invalid:originating_run_unverified")
    );
    assert!(
        warnings
            .iter()
            .any(|w| w == "baseline_miss:originating_run_unverified"),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn lookup_without_repo_scope_misses_before_spawn() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": seed.base, "head": seed.head, "event": "pull_request", "root": repo.path().display().to_string()});
    let value: Json = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    // The exact artifact name always forms from the plan; with no
    // repository scope (no origin, no request slug) the live lookup
    // misses before spawning anything: execute-all with the precise
    // miss reason, never a whole-run download.
    assert_eq!(plan.baseline.reason(), Some("baseline_repo_unresolved"));
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
    let report: velnor_actions_contract_workflow::TaskReport = serde_json::from_value(
        serde_json::json!({"schema": 1, "task_report_id": id, "run_key": "local", "event": "pull_request", "trust": "pr", "matrix_id": entry.id, "matrix_key": entry.matrix_key, "task_id": ob.task_id, "task_digest": ob.task_digest, "status": "executed", "cache": {"layer": "task", "key": "k", "result": "miss", "miss_reason": "no_entry"}, "exit_code": 0, "duration_ms": 1, "outputs": []}),
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
