use super::*;

#[test]
fn pull_request_compares_api_head_with_hook_head_not_tested_merge_commit() -> TestResult<()> {
    let execution = execution_source(true, ROOT_PATH, "f", "f")?;
    let started = started_execution(true, ROOT_PATH, "f", "f", "b")?;

    assert!(started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn pull_request_rejects_a_stored_merge_sha_as_the_head_sha() -> TestResult<()> {
    let execution = execution_source(true, ROOT_PATH, "f", "f")?;
    let started = started_execution(true, ROOT_PATH, "f", "f", "a")?;

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn non_pull_request_still_matches_the_tested_commit() -> TestResult<()> {
    let execution = execution_source(false, ROOT_PATH, "f", "f")?;
    let started = started_execution(false, ROOT_PATH, "f", "f", "b")?;

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn same_repository_reusable_workflow_matches_its_verified_leaf_commit() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let started = started_execution(true, LEAF_PATH, "f", "d", "b")?;

    assert!(started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn reusable_workflow_rejects_missing_or_wrong_leaf_commit() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    for stored_leaf_sha in [None, Some("e")] {
        let started = started_execution_with_leaf_sha(
            true,
            LEAF_PATH,
            "d",
            stored_leaf_sha.map(|value| repeat(value, 40)),
            "b",
        )?;
        assert!(!started.matches_source_request(&execution)?);
    }
    Ok(())
}

#[test]
fn reusable_workflow_rejects_wrong_leaf_path_and_root_only_claim() -> TestResult<()> {
    let reusable = execution_source(true, LEAF_PATH, "f", "d")?;
    let wrong_path = started_execution(true, ROOT_PATH, "f", "d", "b")?;
    assert!(!wrong_path.matches_source_request(&reusable)?);

    let root = execution_source(true, ROOT_PATH, "f", "f")?;
    let wrong_root = started_execution(true, ROOT_PATH, "e", "e", "b")?;
    assert!(!wrong_root.matches_source_request(&root)?);
    Ok(())
}

#[test]
fn native_ref_must_match_its_parsed_workflow_identity() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let mut started = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    let mut source: StoredRequestSource = serde_json::from_str(&started.source_json)?;
    source.workflow.full_ref = workflow_ref(LEAF_PATH, "refs/heads/other");
    started.source_json = serde_json::to_string(&source)?;
    started.source_digest = digest(started.source_json.as_bytes());

    assert!(started.matches_source_request(&execution).is_err());
    Ok(())
}

#[test]
fn native_reusable_workflow_cannot_cross_repository_boundary() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let mut started = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    let mut source: StoredRequestSource = serde_json::from_str(&started.source_json)?;
    source.job_workflow_ref = format!("other/repository/{LEAF_PATH}@refs/heads/reusable-v2");
    source.workflow.full_ref = source.job_workflow_ref.clone();
    source.workflow.owner_name = "other".to_owned();
    source.workflow.repository_name = "repository".to_owned();
    started.source_json = serde_json::to_string(&source)?;
    started.source_digest = digest(started.source_json.as_bytes());

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn native_context_requires_the_exact_started_runner_request() -> TestResult<()> {
    let request: ExecutionSourceRequest =
        serde_json::from_value(request_value(true, &repeat("f", 40)))?;
    let mut started = started_execution(true, ROOT_PATH, "f", "f", "b")?;
    started.jit_runner_id = Some(701);
    started.jit_runner_name = Some("runner-701".to_owned());
    started.jit_started_request_id = Some(102);
    let channel = test_channel_binding()?;

    let context =
        super::super::super::super::native_context::validate_started(&started, &request, &channel)?;
    assert_eq!(
        context.job_workflow_ref(),
        workflow_ref(ROOT_PATH, "refs/pull/9/merge")
    );
    assert_eq!(
        context.workflow_source_sha(),
        Some(repeat("f", 40).as_str())
    );

    let mut reusable = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    reusable.jit_runner_id = Some(701);
    reusable.jit_runner_name = Some("runner-701".to_owned());
    reusable.jit_started_request_id = Some(102);
    let reusable_context = super::super::super::super::native_context::validate_started(
        &reusable, &request, &channel,
    )?;
    assert_eq!(
        reusable_context.job_workflow_ref(),
        workflow_ref(LEAF_PATH, "refs/heads/reusable-v2")
    );
    assert_eq!(
        reusable_context.workflow_source_sha(),
        Some(repeat("d", 40).as_str())
    );

    let mut mismatched = started;
    mismatched.runner_id = 702;
    assert!(
        super::super::super::super::native_context::validate_started(
            &mismatched,
            &request,
            &channel,
        )
        .is_err()
    );
    Ok(())
}
