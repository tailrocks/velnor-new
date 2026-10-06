//! Gate 7 cases: shard proofs, empty partitions, limits, reference sets.

use velnor_actions_contract_workflow::ExecuteTaskRef;
use velnor_actions_contract_workflow::{FinalStatus, MatrixReport, MatrixStatus, Plan, TaskStatus};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};

/// One test identity value: package, target, features, binary, name.
fn test_value(name: &str) -> serde_json::Value {
    serde_json::json!({
        "package": "pkg",
        "target": "lib",
        "features": ["default"],
        "binary": "pkg-test",
        "name": name,
    })
}

/// Inventory digest over canonical sorted test values.
fn inventory_digest(tests: &mut [serde_json::Value]) -> Result<String, Box<dyn std::error::Error>> {
    tests.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let bytes = velnor_actions_contract::canonical_json_bytes(&tests)?;
    Ok(velnor_actions_contract::digest_b3(&bytes))
}

/// One shard proof value for `task` with the given partition.
fn proof_value(
    task: &str,
    input: &str,
    runner: &str,
    index: u32,
    tests: &[serde_json::Value],
    inventory: &str,
) -> serde_json::Value {
    serde_json::json!({
        "task_id": task,
        "input_digest": input,
        "runner": runner,
        "shard_index": index,
        "shard_count": 2,
        "tests": tests,
        "inventory_digest": inventory,
        "archive_digest": velnor_actions_contract::digest_b3(b"archive"),
        "no_test_targets": false,
    })
}

/// Rewrite the first obligation/entry into a two-shard Nextest partition.
fn shard_first_entry(
    plan: &mut Plan,
) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let base = plan.obligations.remove(0);
    let first = format!("{}/shard-1-of-2", base.task_id);
    let second = format!("{}/shard-2-of-2", base.task_id);
    let mut shards = Vec::new();
    for id in [&first, &second] {
        let mut ob = base.clone();
        ob.task_id.clone_from(id);
        ob.task_digest =
            velnor_actions_contract::digest_b3(format!("{}{id}", base.task_digest).as_bytes());
        shards.push(ob);
    }
    plan.obligations.extend(shards);
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
        ExecuteTaskRef::Shards(vec![first.clone(), second.clone()]),
    )]);
    plan.validate()?;
    Ok((base.task_id, first, second))
}

/// Passing reports with per-task status overrides for the sharded entry.
fn sharded_reports(
    plan: &Plan,
    statuses: &[(String, TaskStatus)],
) -> Result<Vec<MatrixReport>, Box<dyn std::error::Error>> {
    let mut trimmed = plan.clone();
    trimmed.matrix.include.remove(0);
    let mut reports = passing_reports(&trimmed)?;
    let entry = plan.matrix.include.first().ok_or("missing entry")?;
    let mut report = MatrixReport {
        schema: 1,
        report_id: entry.report_id.clone(),
        run_key: "local".to_owned(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: Vec::new(),
        task_report_ids: Vec::new(),
        tasks: Vec::new(),
        selected: 0,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    let mut tasks = Vec::new();
    let mut report_ids = Vec::new();
    for (task_id, status) in statuses {
        let ob = plan
            .obligations
            .iter()
            .find(|ob| &ob.task_id == task_id)
            .ok_or("missing obligation")?;
        let id = velnor_actions_contract::task_report_id_for_task(
            "local",
            &entry.matrix_key,
            &ob.task_digest,
        )?;
        report_ids.push(id.clone());
        tasks.push(velnor_actions_contract_workflow::MatrixTaskEntry {
            task_report_id: id,
            task_id: task_id.clone(),
            status: *status,
            exit_code: 0,
        });
    }
    tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    report_ids.sort();
    let mut ids: Vec<String> = statuses.iter().map(|(id, _)| id.clone()).collect();
    ids.sort();
    report.expected_task_ids = ids;
    report.task_report_ids = report_ids;
    report.tasks = tasks;
    report.selected = u32::try_from(statuses.len()).unwrap_or(u32::MAX);
    report.executed = statuses
        .iter()
        .filter(|(_, s)| *s == TaskStatus::Executed)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    report.empty_partition = statuses
        .iter()
        .filter(|(_, s)| *s == TaskStatus::EmptyPartition)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    report.validate()?;
    reports.insert(0, report);
    Ok(reports)
}

/// Merge one plan with proofs and optional limits/reference extras.
fn merge_with(
    plan: &Plan,
    reports: &[MatrixReport],
    proofs: &[serde_json::Value],
    extra: &serde_json::Value,
) -> Result<FinalStatus, Box<dyn std::error::Error>> {
    let plan_value = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
    let reports_value = serde_json::to_value(reports).unwrap_or(serde_json::Value::Null);
    let task_files = crate::impl_merge::task_reports_for(&plan_value, &reports_value);
    let mut request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": plan_value.get("event").cloned().unwrap_or(serde_json::Value::Null),
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": reports,
        "task_reports": task_files,
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
        "shard_proofs": proofs,
    });
    for (key, value) in extra.as_object().ok_or("not an object")? {
        request[key] = value.clone();
    }
    Ok(crate::impl_gates_shard_tokens::merge_report_with(plan, reports, proofs, extra)?.status)
}

/// Canonical sharded plan plus inventory, proofs, and passing reports.
type ShardedCase =
    Result<(Plan, String, Vec<MatrixReport>, Vec<serde_json::Value>), Box<dyn std::error::Error>>;

pub(crate) fn sharded_case() -> ShardedCase {
    let (_repo, mut plan) = plan_for_source_change()?;
    let (_base, first, second) = shard_first_entry(&mut plan)?;
    let input = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == first)
        .ok_or("missing obligation")?
        .input_digest
        .clone();
    let mut all = vec![test_value("one"), test_value("two")];
    let inventory = inventory_digest(&mut all)?;
    let proofs = vec![
        proof_value(
            &first,
            &input,
            "cargo_nextest",
            1,
            &[test_value("one")],
            &inventory,
        ),
        proof_value(
            &second,
            &input,
            "cargo_nextest",
            2,
            &[test_value("two")],
            &inventory,
        ),
    ];
    let reports = sharded_reports(
        &plan,
        &[
            (first, TaskStatus::Executed),
            (second, TaskStatus::Executed),
        ],
    )?;
    Ok((plan, inventory, reports, proofs))
}

#[test]
fn sharded_merge_passes_with_exact_proofs() -> TestResult {
    let (plan, _inventory, reports, proofs) = sharded_case()?;
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &serde_json::json!({}))?,
        FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn duplicate_missing_and_tampered_shards_fail() -> TestResult {
    let (plan, inventory, reports, proofs) = sharded_case()?;
    let mut dup = proofs.clone();
    dup[1]["tests"] = serde_json::json!([test_value("one")]);
    assert_eq!(
        merge_with(&plan, &reports, &dup, &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    assert_eq!(
        merge_with(&plan, &reports, &proofs[..1], &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    assert_eq!(
        merge_with(&plan, &reports, &[], &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    let mut tampered = proofs.clone();
    tampered[1]["tests"] = serde_json::json!([test_value("three")]);
    assert_eq!(
        merge_with(&plan, &reports, &tampered, &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    let mut cargo = proofs.clone();
    cargo[0]["runner"] = serde_json::json!("cargo_test");
    cargo[1]["runner"] = serde_json::json!("cargo_test");
    assert_eq!(
        merge_with(&plan, &reports, &cargo, &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    let _ = inventory;
    Ok(())
}

#[test]
fn empty_partition_needs_inventory_proof() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    let (_base, first, second) = shard_first_entry(&mut plan)?;
    let input = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == first)
        .ok_or("missing obligation")?
        .input_digest
        .clone();
    let mut all = vec![test_value("one")];
    let inventory = inventory_digest(&mut all)?;
    let proofs = vec![
        proof_value(
            &first,
            &input,
            "cargo_nextest",
            1,
            &[test_value("one")],
            &inventory,
        ),
        proof_value(&second, &input, "cargo_nextest", 2, &[], &inventory),
    ];
    let reports = sharded_reports(
        &plan,
        &[
            (first.clone(), TaskStatus::Executed),
            (second.clone(), TaskStatus::EmptyPartition),
        ],
    )?;
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &serde_json::json!({}))?,
        FinalStatus::Passed
    );
    let unproven = sharded_reports(
        &plan,
        &[
            (first.clone(), TaskStatus::Executed),
            (second.clone(), TaskStatus::Executed),
        ],
    )?;
    assert_eq!(
        merge_with(&plan, &unproven, &proofs, &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    Ok(())
}

#[test]
fn proven_no_target_shard_passes_without_tests() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    let (_base, first, second) = shard_first_entry(&mut plan)?;
    let single = first.replace("shard-1-of-2", "shard-1-of-1");
    plan.obligations.retain(|ob| ob.task_id != second);
    for ob in &mut plan.obligations {
        if ob.task_id == first {
            ob.task_id.clone_from(&single);
        }
    }
    plan.task_ids = plan
        .obligations
        .iter()
        .map(|ob| ob.task_id.clone())
        .collect();
    let entry = plan.matrix.include.first_mut().ok_or("missing entry")?;
    entry.execute_task_ids.tasks = std::collections::BTreeMap::from([(
        "nextest".to_owned(),
        ExecuteTaskRef::Shards(vec![single.clone()]),
    )]);
    plan.validate()?;
    let input = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == single)
        .ok_or("missing ob")?
        .input_digest
        .clone();
    let empty: [serde_json::Value; 0] = [];
    let inventory =
        velnor_actions_contract::digest_b3(&velnor_actions_contract::canonical_json_bytes(&empty)?);
    let mut proof = proof_value(&single, &input, "cargo_nextest", 1, &[], &inventory);
    proof["shard_count"] = serde_json::json!(1);
    proof["no_test_targets"] = serde_json::json!(true);
    proof["archive_digest"] = serde_json::Value::Null;
    let reports = sharded_reports(&plan, &[(single, TaskStatus::Executed)])?;
    assert_eq!(
        merge_with(&plan, &reports, &[proof], &serde_json::json!({}))?,
        FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn archive_without_shard_proofs_never_passes() -> TestResult {
    let (plan, _inventory, reports, _proofs) = sharded_case()?;
    assert_eq!(
        merge_with(&plan, &reports, &[], &serde_json::json!({}))?,
        FinalStatus::PlanningFailed
    );
    Ok(())
}

#[test]
fn limits_and_reference_revalidate_at_merge() -> TestResult {
    let (plan, _inventory, reports, proofs) = sharded_case()?;
    let good = serde_json::json!({"compiler_budget": 4, "test_budget": 4, "max_parallel": 4, "capacity": 8, "shards": 2, "retries": 0});
    let extra = serde_json::json!({"limits": good, "reference_task_ids": plan.task_ids});
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &extra)?,
        FinalStatus::Passed
    );
    let retry = serde_json::json!({"limits": {"compiler_budget": 4, "test_budget": 4, "max_parallel": 4, "capacity": 8, "shards": 2, "retries": 1}});
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &retry)?,
        FinalStatus::PlanningFailed
    );
    let over = serde_json::json!({"limits": {"compiler_budget": 4, "test_budget": 1, "max_parallel": 4, "capacity": 8, "shards": 2, "retries": 0}});
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &over)?,
        FinalStatus::PlanningFailed
    );
    let wrong_ref = serde_json::json!({"reference_task_ids": ["stack/rust/root/clippy/default"]});
    assert_eq!(
        merge_with(&plan, &reports, &proofs, &wrong_ref)?,
        FinalStatus::PlanningFailed
    );
    Ok(())
}
