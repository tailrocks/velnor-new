use super::*;
use std::collections::BTreeMap;

fn environment(phase: Option<&str>) -> BTreeMap<String, String> {
    let mut values = BTreeMap::from([
        (RUNNER_TEMP_ENV.to_owned(), "/tmp/runner".to_owned()),
        ("GITHUB_RUN_ID".to_owned(), "42".to_owned()),
        ("GITHUB_RUN_ATTEMPT".to_owned(), "3".to_owned()),
        ("GITHUB_REF_PROTECTED".to_owned(), "true".to_owned()),
        (OWNER_ID_ENV.to_owned(), "job-owner".to_owned()),
        (LAYER_ENV.to_owned(), "cargo_sources".to_owned()),
        ("GITHUB_EVENT_NAME".to_owned(), "push".to_owned()),
        ("GITHUB_REPOSITORY".to_owned(), "owner/repo".to_owned()),
        ("GITHUB_REF".to_owned(), "refs/heads/main".to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        ),
        ("GITHUB_WORKFLOW_SHA".to_owned(), "a".repeat(40)),
        ("GITHUB_SHA".to_owned(), "b".repeat(40)),
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
    ]);
    if let Some(phase) = phase {
        values.insert(RUNTIME_PHASE_ENV.to_owned(), phase.to_owned());
    }
    values
}

#[test]
fn runtime_phase_selects_only_its_canonical_identity_path() {
    let before_env = environment(Some("before_import"));
    let before = read_inputs(|name| {
        before_env
            .get(name)
            .cloned()
            .ok_or(env::VarError::NotPresent)
    })
    .expect("before-import inputs");
    assert!(
        before
            .runtime_identity_path
            .ends_with("runtime-identity-job-owner-cargo_sources.json")
    );

    let after_env = environment(Some("after_import"));
    let after = read_inputs(|name| {
        after_env
            .get(name)
            .cloned()
            .ok_or(env::VarError::NotPresent)
    })
    .expect("after-import inputs");
    assert!(
        after
            .runtime_identity_path
            .ends_with("runtime-identity-job-owner-cargo_sources-after-import.json")
    );
}

#[test]
fn runtime_phase_is_required_and_closed() {
    let missing_env = environment(None);
    let missing = read_inputs(|name| {
        missing_env
            .get(name)
            .cloned()
            .ok_or(env::VarError::NotPresent)
    });
    assert_eq!(
        missing.expect_err("phase is required"),
        "missing_velnor_qualification_cache_runtime_phase"
    );

    let mut invalid_env = environment(Some("before_import"));
    invalid_env.insert(RUNTIME_PHASE_ENV.to_owned(), "before-import".to_owned());
    let invalid = read_inputs(|name| {
        invalid_env
            .get(name)
            .cloned()
            .ok_or(env::VarError::NotPresent)
    });
    assert_eq!(invalid.expect_err("phase is closed"), "bad_runtime_phase");
}
