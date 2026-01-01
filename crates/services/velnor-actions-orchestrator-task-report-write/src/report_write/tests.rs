use super::parse_downstream;

#[test]
fn downstream_ids_split_dedupe_and_drop_blanks() {
    assert_eq!(parse_downstream(None), Vec::<String>::new());
    assert_eq!(parse_downstream(Some("")), Vec::<String>::new());
    assert_eq!(parse_downstream(Some("b,a,b,, a ,")), ["b", "a"]);
}

#[test]
fn runtime_identity_is_required_and_bound_for_github_runs() {
    use std::collections::BTreeMap;

    let fields = BTreeMap::from([
        ("GITHUB_REPOSITORY", "org/repo"),
        ("GITHUB_RUN_ID", "123"),
        ("GITHUB_RUN_ATTEMPT", "2"),
        ("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        (
            "GITHUB_WORKFLOW_REF",
            "org/repo/.github/workflows/ci.yml@refs/heads/main",
        ),
        ("GITHUB_JOB", "crate_demo"),
        ("RUNNER_NAME", "runner-17"),
    ]);
    let identity = super::runtime_identity_for_run_key("r123-a2", |name| {
        fields.get(name).map(|value| (*value).to_owned())
    })
    .expect("complete context")
    .expect("GitHub context");
    assert_eq!(identity.repository, "org/repo");
    assert_eq!(identity.run_id, "123");
    assert_eq!(identity.run_attempt, 2);
    assert_eq!(identity.workflow_job_key, "crate_demo");
    assert_eq!(identity.runner_name, "runner-17");
    assert!(
        super::runtime_identity_for_run_key("r123-a1", |name| {
            fields.get(name).map(|value| (*value).to_owned())
        })
        .expect_err("attempt mismatch must fail closed")
        .to_string()
        .contains("runtime_run_key_mismatch")
    );
}

#[test]
fn local_report_does_not_require_github_runtime_variables() {
    let identity = super::runtime_identity_for_run_key("local", |_| {
        panic!("local reports must not read GitHub runtime values")
    })
    .expect("local context");
    assert!(identity.is_none());
}

#[test]
fn github_report_rejects_missing_runtime_job_identity() {
    let fields = std::collections::BTreeMap::from([
        ("GITHUB_REPOSITORY", "org/repo"),
        ("GITHUB_RUN_ID", "123"),
        ("GITHUB_RUN_ATTEMPT", "2"),
        ("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        (
            "GITHUB_WORKFLOW_REF",
            "org/repo/.github/workflows/ci.yml@refs/heads/main",
        ),
        ("RUNNER_NAME", "runner-17"),
    ]);
    let error = super::runtime_identity_for_run_key("r123-a2", |name| {
        fields.get(name).map(|value| (*value).to_owned())
    })
    .expect_err("missing job key must fail closed");
    assert!(
        error
            .to_string()
            .contains("missing_runtime_identity:GITHUB_JOB")
    );
}
