//! Request-envelope strictness: `plan-v1`/`merge-v1` reject duplicate keys.

use velnor_actions_orchestrator::{merge_internal, plan_internal};

use crate::impl_common::{TestResult, err_of};

#[test]
fn plan_envelope_rejects_duplicate_keys() -> TestResult {
    let dup = r#"{"schema":1,"base":null,"head":"h","event":"push","schema":1}"#;
    let err = err_of(plan_internal(dup), "dup plan envelope")?;
    assert!(
        err.to_string().contains("duplicate_key:schema"),
        "got {err}"
    );
    let nested = r#"{"schema":1,"base":null,"head":"h","event":"push","generator":{"service":"a","service":"b"}}"#;
    let err = err_of(plan_internal(nested), "nested dup plan envelope")?;
    assert!(
        err.to_string().contains("duplicate_key:service"),
        "got {err}"
    );
    let err = err_of(plan_internal(r#"{"schema":1}"#), "short plan")?;
    assert!(
        err.to_string().contains("malformed_request"),
        "typed decode still enforced: {err}"
    );
    let err = err_of(
        plan_internal(r#"{"schema":1,"run_key":"local","base":null,"head":"","event":"push"}"#),
        "empty head",
    )?;
    assert!(
        err.to_string().contains("empty_head"),
        "dup-free envelope parses through: {err}"
    );
    Ok(())
}

#[test]
fn merge_envelope_rejects_duplicate_keys() -> TestResult {
    let dup = r#"{"schema":1,"run_key":"local","matrix_reports":[],"required_job_ids":[],"required_jobs":[],"schema":1}"#;
    let err = err_of(merge_internal(dup), "dup merge envelope")?;
    assert!(
        err.to_string().contains("duplicate_key:schema"),
        "got {err}"
    );
    let nested = r#"{"schema":1,"run_key":"local","matrix_reports":[],"required_job_ids":[],"required_jobs":[],"plan":{"run_key":"local","run_key":"other"}}"#;
    let err = err_of(merge_internal(nested), "nested dup merge envelope")?;
    assert!(
        err.to_string().contains("duplicate_key:run_key"),
        "got {err}"
    );
    let err = err_of(merge_internal(r#"{"schema":1}"#), "short merge")?;
    assert!(
        err.to_string().contains("malformed_request"),
        "typed decode still enforced: {err}"
    );
    let merged = merge_internal(
        r#"{"schema":1,"run_key":"local","matrix_reports":[],"required_job_ids":[],"required_jobs":[]}"#,
    )?;
    assert!(
        merged.contains("planning_failed"),
        "dup-free envelope merges to a diagnostic: {merged}"
    );
    Ok(())
}
