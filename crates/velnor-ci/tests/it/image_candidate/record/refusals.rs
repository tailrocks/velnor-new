use super::*;

#[test]
fn every_malformed_record_field_is_refused_with_its_own_message() {
    let cases = [
        top_and_source_cases(),
        workflow_cases(),
        build_archive_and_oci_cases(),
    ]
    .concat();
    assert!(cases.len() >= 40, "{} cases", cases.len());
    for (path, value, message) in cases {
        assert_eq!(
            verdict(&with(&path, value.clone())),
            message,
            "{path:?} {value:?}"
        );
    }
}

#[test]
fn the_github_environment_becomes_the_recorded_workflow_identity() {
    let (identity, reference) = workflow(&env()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(Value::Object(identity), record()["workflow"]);
    assert_eq!(reference, "refs/heads/candidate");
}

#[test]
fn a_missing_or_malformed_github_variable_is_refused() {
    let cases = [
        (
            "GITHUB_REF",
            "heads/x",
            "GitHub source ref is missing or incorrect.",
        ),
        (
            "GITHUB_REPOSITORY",
            "bad",
            "GitHub repository identity is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "0",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "-1",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "1.5",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_EVENT_NAME",
            "PUSH",
            "GitHub event name is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/x@refs/a",
            "GitHub workflow ref is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_SHA",
            "z",
            "GitHub workflow authority commit is incorrect.",
        ),
        ("GITHUB_RUN_ID", "0", "GitHub run_id is incorrect."),
        (
            "GITHUB_RUN_ATTEMPT",
            "x",
            "GitHub run_attempt is incorrect.",
        ),
    ];
    for (name, value, message) in cases {
        let mut vars = env();
        vars.insert(name.to_owned(), value.to_owned());
        assert_eq!(
            workflow(&vars).err().map(|e| e.to_string()).as_deref(),
            Some(message),
            "{name}={value}"
        );
        vars.remove(name);
        assert!(workflow(&vars).is_err(), "{name} missing");
    }
}
