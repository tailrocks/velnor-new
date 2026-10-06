//! Downstream authority rejects forged predecessors, gaps, and reordered suffixes.

use std::collections::BTreeMap;

use velnor_actions_contract::{BaselineProof, ExecuteTaskIds, ExecuteTaskRef, ObligationDecision};

use super::*;

/// Build a valid single-owner selected plan independent of lexical matrix order.
fn fixture(stack: &str, kinds: &[&str]) -> Plan {
    let mut plan = crate::task_report::task_report_tests::fixture_plan();
    let template = plan.obligations[0].clone();
    plan.matrix.include.clear();
    plan.obligations.clear();
    plan.task_ids.clear();
    for kind in kinds {
        let id = format!("stack/{stack}/demo/{kind}/default");
        plan.matrix.include.push(
            MatrixEntry::derive(
                stack,
                &id,
                "true",
                &template.task_digest,
                serde_json::json!({}),
                ExecuteTaskIds {
                    tasks: BTreeMap::from([((*kind).into(), ExecuteTaskRef::Single(id.clone()))]),
                },
                &template.input_digest,
                &plan.run_key,
                "native-demo",
            )
            .expect("entry"),
        );
        let mut obligation = template.clone();
        obligation.task_id = id.clone();
        obligation.job_id = "native-demo".into();
        plan.obligations.push(obligation);
        plan.task_ids.push(id);
    }
    plan.matrix.include.sort_by(|a, b| a.id.cmp(&b.id));
    plan.obligations.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    plan.task_ids.sort();
    plan.validate().expect("valid fixture");
    plan
}

fn entry<'a>(plan: &'a Plan, kind: &str) -> &'a MatrixEntry {
    plan.matrix
        .include
        .iter()
        .find(|entry| crate::extension_schemas::task_kind_segment(&entry.task_id) == Some(kind))
        .expect("fixture source")
}

fn ids(plan: &Plan, kinds: &[&str]) -> Vec<String> {
    kinds
        .iter()
        .map(|kind| entry(plan, kind).task_id.clone())
        .collect()
}

#[test]
fn workload_requires_complete_generator_suffix_in_order() {
    let plan = fixture("workload", &["install", "build", "test"]);
    let source = entry(&plan, "install");
    check(&plan, source, &ids(&plan, &["build", "test"])).expect("complete suffix");
    for forged in [
        vec![],
        ids(&plan, &["test"]),
        ids(&plan, &["test", "build"]),
        ids(&plan, &["build", "build", "test"]),
        ids(&plan, &["install", "build", "test"]),
    ] {
        assert!(check(&plan, source, &forged).is_err(), "reject {forged:?}");
    }
    assert!(check(&plan, entry(&plan, "test"), &ids(&plan, &["build"])).is_err());
    check(&plan, entry(&plan, "test"), &[]).expect("last suffix empty");
}

#[test]
fn tofu_uses_generator_rank_and_rejects_prior_same_job_tasks() {
    let plan = fixture("tofu", &["fmt", "init", "validate"]);
    let source = entry(&plan, "init");
    check(&plan, source, &ids(&plan, &["validate"])).expect("tofu successor");
    assert!(check(&plan, source, &ids(&plan, &["fmt", "validate"])).is_err());
}

#[test]
fn equal_rank_obligations_follow_generator_task_identity_order() {
    let plan = fixture("workload", &["install", "syntax", "shellcheck", "test"]);
    let source = entry(&plan, "install");
    check(
        &plan,
        source,
        &ids(&plan, &["shellcheck", "syntax", "test"]),
    )
    .expect("rank ties sort by task ID");
    assert!(
        check(
            &plan,
            source,
            &ids(&plan, &["syntax", "shellcheck", "test"])
        )
        .is_err()
    );
    assert!(
        check(
            &plan,
            entry(&plan, "syntax"),
            &ids(&plan, &["shellcheck", "test"])
        )
        .is_err()
    );
}

/// Covered obligations retain generator ownership even without matrix entries.
fn cover(plan: &mut Plan, kind: &str) -> String {
    let id = entry(plan, kind).task_id.clone();
    let commit = "a".repeat(40);
    let compat = velnor_actions_contract::digest_b3(b"compat");
    let name = format!("velnor-baseline-{commit}-{compat}");
    let proof = BaselineProof::new(
        &commit,
        7,
        crate::cover_compat::baseline_artifact_numeric_id(&name),
        &name,
        &velnor_actions_contract::digest_b3(b"manifest"),
    )
    .expect("proof");
    let obligation = plan
        .obligations
        .iter_mut()
        .find(|obligation| obligation.task_id == id)
        .expect("covered obligation");
    obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
    obligation.baseline_proof = Some(proof);
    plan.matrix.include.retain(|entry| entry.task_id != id);
    plan.validate().expect("covered plan");
    id
}

#[test]
fn covered_later_ids_are_vacuous_but_cannot_authorize_prior_or_foreign_tasks() {
    let mut plan = fixture("workload", &["install", "build", "test"]);
    let build = cover(&mut plan, "build");
    let source = entry(&plan, "install");
    let test = entry(&plan, "test").task_id.clone();
    check(&plan, source, std::slice::from_ref(&test)).expect("selected suffix");
    check(&plan, source, &[build.clone(), test.clone()]).expect("covered suffix");
    assert!(check(&plan, source, &[build.clone(), build.clone(), test]).is_err());
    assert!(check(&plan, entry(&plan, "test"), std::slice::from_ref(&build)).is_err());
    plan.obligations
        .iter_mut()
        .find(|obligation| obligation.task_id == build)
        .expect("covered obligation")
        .job_id = "foreign-job".into();
    assert!(
        check(
            &plan,
            entry(&plan, "install"),
            &[build, entry(&plan, "test").task_id.clone()]
        )
        .is_err()
    );
}

#[test]
fn foreign_selected_tasks_and_missing_sources_have_no_skip_authority() {
    let mut plan = fixture("workload", &["install", "build", "test"]);
    let source = entry(&plan, "install").clone();
    let test = entry(&plan, "test").task_id.clone();
    plan.matrix
        .include
        .iter_mut()
        .find(|entry| entry.task_id == test)
        .expect("test entry")
        .job_id = "foreign-job".into();
    assert!(check(&plan, &source, &ids(&plan, &["build", "test"])).is_err());
    plan.matrix
        .include
        .retain(|entry| entry.task_id != source.task_id);
    assert!(check(&plan, &source, &ids(&plan, &["build"])).is_err());
}
