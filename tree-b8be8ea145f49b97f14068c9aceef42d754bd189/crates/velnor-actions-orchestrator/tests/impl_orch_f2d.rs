//! F2 plan/merge acceptance: offline, omission, cache, broaden, baseline.

use std::collections::BTreeSet;

use serde_json::Value as Json;
use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::{merge_internal, plan_internal};

use crate::impl_common::{
    TestResult, config_with_branch, git, install_fixture_release_manifest, make_repo,
    passing_reports, plan_for_source_change,
};
use crate::impl_orch_plansel::{
    BUMP, anchor_repo, commit, entries_for, has, make_ws, manifest_for, merge_status, plan_at,
    plan_change, put,
};

#[test]
fn offline_dependency_aborts_plan() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nvelnor-nonexistent-crate-xyz = \"9.9.9\"\n",
    )?;
    std::fs::write(repo.path().join("Cargo.lock"), "version = 3\n")?;
    let head = commit(repo.path(), "one")?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": None::<String>, "head": head, "event": "push", "root": repo.path().display().to_string()});
    let err = plan_internal(&request.to_string()).expect_err("offline dep must abort");
    let text = err.to_string();
    assert!(text.contains("preparation_incomplete"), "{text}");
    assert!(text.contains("Cargo.toml"), "{text}");
    assert!(text.contains("metadata_offline"), "{text}");
    Ok(())
}

#[test]
fn obligations_carry_internal_reasons() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let base = commit(repo.path(), "one")?;
    put(repo.path(), "beta/src/lib.rs", BUMP)?;
    let head = commit(repo.path(), "two")?;
    let (full, _) = plan_at(repo.path(), None, &head, None)?;
    let (compared, _) = plan_at(repo.path(), Some(&base), &head, None)?;
    assert!(has(&full, "alpha") && has(&full, "beta"));
    assert_eq!(
        full.task_ids, compared.task_ids,
        "universe stable across comparisons"
    );
    assert!(
        full.obligations
            .iter()
            .all(|ob| ob.reason == "affected_by_change"),
        "unknown comparison marks all changed: {:?}",
        full.obligations
    );
    for obligation in compared.obligations.iter().chain(full.obligations.iter()) {
        assert!(!obligation.reason.is_empty(), "{}", obligation.task_id);
    }
    Ok(())
}

#[test]
fn plan_digests_combine_deterministically() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let base = seed.base.clone().ok_or("missing base")?;
    let (again, _) = plan_at(repo.path(), Some(&base), &seed.head, None)?;
    let digests = |plan: &Plan| {
        plan.obligations
            .iter()
            .map(|ob| (ob.task_digest.clone(), ob.input_digest.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(digests(&seed), digests(&again), "stable across runs");
    let tasks: BTreeSet<&str> = seed
        .obligations
        .iter()
        .map(|ob| ob.task_digest.as_str())
        .collect();
    let inputs: BTreeSet<&str> = seed
        .obligations
        .iter()
        .map(|ob| ob.input_digest.as_str())
        .collect();
    assert_eq!(tasks.len(), seed.obligations.len(), "task digests distinct");
    assert_eq!(
        inputs.len(),
        seed.obligations.len(),
        "input digests distinct"
    );
    Ok(())
}

#[test]
fn cache_miss_cannot_fail_merge() -> TestResult {
    use velnor_actions_contract::{CacheLayer, CacheResult, TaskReport};
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    // Strict contract: `MatrixReport` carries no `cache` field, so the
    // old staple-and-tolerate reparse is rejected. Cache-miss validity
    // is pinned below on a real `TaskReport` with `CacheOutcome::Miss`.
    assert_eq!(
        merge_status(&plan, &reports, &[], &serde_json::json!({}))?,
        FinalStatus::Passed
    );
    let entry = &plan.matrix.include[0];
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == entry.task_id)
        .ok_or("ob")?;
    let inputs = velnor_actions_orchestrator::decisions::NotSelectedInputs {
        run_key: "local",
        event: velnor_actions_contract::WorkflowEvent::PullRequest,
        trust: velnor_actions_contract::Trust::Pr,
        matrix_id: &entry.id,
        planned_platform: &entry.planned_platform,
        matrix_key: &entry.matrix_key,
        task_id: &entry.task_id,
        task_digest: &obligation.task_digest,
        reason: velnor_actions_contract::NotSelectedReason::UpstreamFailed,
    };
    let mut report: TaskReport =
        velnor_actions_orchestrator::decisions::not_selected_report(&inputs)?;
    report.status = velnor_actions_contract::TaskStatus::Executed;
    report.not_selected_reason = None;
    report.cache = velnor_actions_contract::CacheOutcome {
        layer: CacheLayer::Task,
        key: "k".to_owned(),
        result: CacheResult::Miss,
        miss_reason: Some("no_entry".to_owned()),
    };
    report.validate()?;
    Ok(())
}

#[test]
fn merge_without_reports_is_not_run() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    let request = serde_json::json!({"schema": 1, "run_key": "local", "actual_event": "pull_request", "plan": plan, "matrix": plan.matrix, "matrix_reports": [], "required_job_ids": ["plan"], "required_jobs": [{"job_id": "plan", "conclusion": "success"}]});
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request.to_string())?)?;
    assert_eq!(final_report.status, FinalStatus::NotRun);
    assert_eq!(
        final_report.counts.not_run as usize,
        plan.matrix.include.len()
    );
    Ok(())
}

#[test]
fn undetected_stacks_plan_no_work() -> TestResult {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "t@e.c"], root)?;
    git(&["config", "user.name", "T"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    std::fs::create_dir_all(root.join(".velnor"))?;
    std::fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    install_fixture_release_manifest(root)?;
    std::fs::write(root.join("README.md"), "no manifests here\n")?;
    let head = commit(root, "one")?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": None::<String>, "head": head, "event": "push", "root": root.display().to_string()});
    let response = plan_internal(&request.to_string())?;
    let value: Json = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert!(plan.task_ids.is_empty(), "no inventory, no work");
    assert!(plan.packages.is_empty());
    let jobs = serde_json::json!([{"job_id": "plan", "conclusion": "success"}, {"job_id": "actionlint", "conclusion": "success"}]);
    let merge = serde_json::json!({"schema": 1, "run_key": "local", "actual_event": "push", "plan": plan, "matrix": plan.matrix, "matrix_reports": [], "required_job_ids": ["plan", "actionlint"], "required_jobs": jobs});
    let merged = merge_internal(&merge.to_string())?;
    let final_report: velnor_actions_contract::FinalReport = serde_json::from_str(&merged)?;
    assert_eq!(final_report.status, FinalStatus::NoWork);
    assert!(
        velnor_actions_orchestrator::merge_passed(&merged)?,
        "no work with valid required checks passes"
    );
    Ok(())
}

#[test]
fn plans_are_byte_deterministic() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let base = seed.base.clone().ok_or("missing base")?;
    let (again, _) = plan_at(repo.path(), Some(&base), &seed.head, None)?;
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&seed)?,
        velnor_actions_contract::canonical_json_bytes(&again)?
    );
    Ok(())
}

#[test]
fn missing_base_broadens_with_warning() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let head = commit(repo.path(), "one")?;
    let (plan, warnings) = plan_at(repo.path(), None, &head, None)?;
    assert!(has(&plan, "alpha") && has(&plan, "beta"), "selects all");
    assert!(
        warnings.iter().any(|w| w.contains("missing_base")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn test_only_changes_still_propagate() -> TestResult {
    let (plan, _) = plan_change(
        &["alpha", "beta"],
        &[("beta", "alpha", "dependencies")],
        &[],
        &[],
        &[("alpha/tests/smoke.rs", "#[test]\nfn t() {}\n")],
    )?;
    assert!(has(&plan, "alpha"), "owner: {:?}", plan.task_ids);
    assert!(has(&plan, "beta"), "consumer: {:?}", plan.task_ids);
    Ok(())
}

#[test]
fn global_config_changes_broaden_explicitly() -> TestResult {
    let tuned = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n# tuned\n";
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[(".velnor/config.toml", tuned)],
    )?;
    assert!(has(&plan, "alpha") && has(&plan, "beta"));
    assert!(
        warnings.iter().any(|w| w.contains("global_config_changed")),
        "{warnings:?}"
    );
    let (plan, warnings) = plan_change(
        &["alpha", "beta"],
        &[],
        &[],
        &[],
        &[(".github/workflows/ci.yml", "name: ci\n")],
    )?;
    assert!(has(&plan, "alpha") && has(&plan, "beta"));
    assert!(
        warnings.iter().any(|w| w.contains("global_config_changed")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn generator_mismatch_executes_with_reason() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let base = seed.base.clone().ok_or("missing base")?;
    let mut manifest = manifest_for(&seed, &base, &entries_for(&seed));
    manifest["generator_version"] = Json::String("9.9.9".to_owned());
    let (plan, warnings) = plan_at(repo.path(), Some(&base), &seed.head, Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(!plan.matrix.include.is_empty(), "executes");
    assert!(
        warnings.iter().any(|w| w.contains("generator_mismatch")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn carried_proofs_execute_without_originating_attestation() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    anchor_repo(repo.path())?;
    let head = seed.head.clone();
    let (nodiff, _) = plan_at(repo.path(), Some(&head), &head, None)?;
    let mut tasks = entries_for(&nodiff);
    for task in tasks.as_array_mut().ok_or("tasks")? {
        task["proof_run_id"] = Json::from(5);
        // Observed by run 7, but observation is not success attestation.
        task["observed_run_id"] = Json::from(7);
    }
    let (plan, warnings) = plan_at(
        repo.path(),
        Some(&head),
        &head,
        Some(manifest_for(&nodiff, &head, &tasks)),
    )?;
    for ob in &plan.obligations {
        assert_eq!(ob.decision, ObligationDecision::Execute);
    }
    let reason = "baseline_invalid:originating_run_unverified";
    assert_eq!(plan.baseline.reason(), Some(reason));
    let missed = "baseline_miss:originating_run_unverified";
    assert!(warnings.iter().any(|w| w == missed));
    Ok(())
}

#[test]
fn shard_budgets_reject_at_plan_time() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[resources]\ncompiler_process_budget = 2\ntest_process_budget = 2\n[test_sharding]\ndefault_shards = 99\n";
    let repo = make_repo(config)?;
    let head = commit(repo.path(), "one")?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": None::<String>, "head": head, "event": "push", "root": repo.path().display().to_string()});
    let err = plan_internal(&request.to_string()).expect_err("over-budget shards rejected");
    assert!(
        err.to_string().contains("exceeds_test_process_budget"),
        "{err}"
    );
    Ok(())
}

#[test]
fn expired_baselines_schedule_execution() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    anchor_repo(repo.path())?;
    let base = seed.base.clone().ok_or("missing base")?;
    let mut manifest = manifest_for(&seed, &base, &entries_for(&seed));
    manifest["expires_at_unix"] = Json::from(1);
    let (plan, warnings) = plan_at(repo.path(), Some(&base), &seed.head, Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(
        warnings.iter().any(|w| w.contains("cache_expired")),
        "{warnings:?}"
    );
    let head = seed.head.clone();
    let (nodiff, _) = plan_at(repo.path(), Some(&head), &head, None)?;
    let mut fresh = manifest_for(&nodiff, &head, &entries_for(&nodiff));
    fresh["expires_at_unix"] = Json::from(4_102_444_800_u64);
    let (covered, _) = plan_at(repo.path(), Some(&head), &head, Some(fresh))?;
    assert!(
        covered
            .obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
    );
    Ok(())
}

#[test]
fn entry_cache_ids_recorded_per_lane() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    let mut lanes = BTreeSet::new();
    for entry in &plan.matrix.include {
        entry.validate("local")?;
        let ids = entry.cache_ids.as_ref().ok_or("cache ids recorded")?;
        ids.validate()?;
        assert!(lanes.insert(ids.lane_id().to_owned()), "lanes distinct");
    }
    let first = plan.matrix.include[0].cache_ids.as_ref().ok_or("ids")?;
    for entry in &plan.matrix.include {
        let ids = entry.cache_ids.as_ref().ok_or("ids")?;
        assert_eq!(ids.toolchain_id(), first.toolchain_id());
        assert_eq!(ids.platform_id(), first.platform_id());
        assert_eq!(ids.cache_format_id(), first.cache_format_id());
        assert_eq!(ids.workspace_id(), first.workspace_id());
    }
    Ok(())
}

#[test]
fn adapter_metadata_forwards_adapter_value() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    for entry in &plan.matrix.include {
        let meta = entry.adapter_metadata.as_object().ok_or("opaque object")?;
        for key in [
            "package_id",
            "package_name",
            "manifest_key",
            "kind",
            "configuration",
            "target",
            "compile_driver",
            "test_runner",
            "evidence_ids",
            "task_cache_enabled",
            "cargo_target_dir",
        ] {
            assert!(meta.contains_key(key), "{key} in {}", entry.id);
        }
        assert_eq!(meta.len(), 11, "adapter shape only");
        assert_ne!(meta["compile_driver"].as_str().unwrap_or_default(), "");
        assert_ne!(meta["test_runner"].as_str().unwrap_or_default(), "");
        assert!(meta["evidence_ids"].is_array());
    }
    Ok(())
}
