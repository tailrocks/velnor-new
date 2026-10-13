use super::*;
use tempfile::TempDir;

/// Stage one `plan.json` under a fake runner temp.
fn stage(text: &str) -> TempDir {
    let dir = TempDir::new().expect("temp");
    let run = dir.path().join("velnor").join("local");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), text).expect("plan");
    dir
}

#[test]
fn plan_read_is_bounded_and_duplicate_rejecting() {
    let valid = serde_json::to_string(&fixture_plan()).expect("valid plan");
    assert!(load_plan("local", stage(&valid).path()).is_ok());
    let mut dup = valid;
    dup.pop();
    dup.push_str(r#","schema":1}"#);
    let err = load_plan("local", stage(&dup).path()).expect_err("dup keys reject");
    assert!(err.to_string().contains("unparsable_plan"), "{err}");
    let bound = usize::try_from(crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES)
        .expect("bound fits pointer width");
    let err =
        load_plan("local", stage(&" ".repeat(bound + 1)).path()).expect_err("oversize rejects");
    assert!(err.to_string().contains("oversize"), "{err}");
}
