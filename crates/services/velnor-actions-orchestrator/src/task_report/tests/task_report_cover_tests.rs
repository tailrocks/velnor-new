//! Covered-obligation report tests: proven by manifest, never reported.
//!
//! Declared via `#[path]` from `task_report.rs` under `cfg(test)`; the
//! parsing/behavior file is at the size gate. Fixtures reuse the
//! sibling `task_report_tests` builders.

use super::*;

use velnor_actions_contract_workflow::{BaselineProof, ObligationDecision};

/// Fixture plan with `TEST` covered and pruned from the matrix.
fn covered_fixture() -> Plan {
    let mut plan = fixture_plan();
    let commit = "a".repeat(40);
    let compat = digest_b3_for("compat");
    let name = format!("velnor-baseline-{commit}-{compat}");
    let numeric =
        velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(&name);
    let proof = BaselineProof::new(&commit, 7, numeric, &name, &digest_b3_for("manifest"))
        .expect("proof constructs");
    for obligation in &mut plan.obligations {
        if obligation.task_id == TEST {
            obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
            obligation.reason = "covered_by_trusted_baseline".to_owned();
            obligation.baseline_proof = Some(proof.clone());
        }
    }
    plan.matrix.include.retain(|entry| entry.task_id != TEST);
    plan.validate().expect("covered fixture validates");
    plan
}

/// One `b3-` digest over `bytes`.
fn digest_b3_for(bytes: &str) -> String {
    velnor_actions_contract::digest_b3(bytes.as_bytes())
}

/// Count staged `matrix-report.json` files under `runner_temp`.
fn staged_report_count(runner_temp: &std::path::Path) -> usize {
    let mut count = 0;
    let mut stack = vec![runner_temp.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name == "matrix-report.json")
            {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn covered_obligation_reports_nothing_and_succeeds() {
    let plan = covered_fixture();
    let temp = staged_run(&plan, "local");
    let reported =
        write_task_report_to("local", TEST, 0, None, &[], temp.path()).expect("covered succeeds");
    assert_eq!(reported, 0);
    assert_eq!(
        staged_report_count(temp.path()),
        0,
        "covered obligations write no report files"
    );
}

#[test]
fn covered_downstream_skips_silently_behind_failure() {
    let plan = covered_fixture();
    let temp = staged_run(&plan, "local");
    // CLIPPY fails with covered TEST downstream: the failure still
    // reports itself, and the covered skip never surfaces as an error.
    let reported = write_task_report_to("local", CLIPPY, 1, None, &[TEST.to_owned()], temp.path())
        .expect("failure with covered downstream reports");
    assert_eq!(reported, 1);
    let dir = temp
        .path()
        .join("velnor")
        .join("local")
        .join(&plan.matrix.include[0].matrix_key);
    assert!(
        dir.join("matrix-report.json").is_file(),
        "the failed obligation still reports"
    );
}
