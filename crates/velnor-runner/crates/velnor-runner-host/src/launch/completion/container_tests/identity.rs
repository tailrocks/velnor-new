//! Container identity and retry tests.

use super::*;

#[tokio::test]
async fn retry_accepts_runner_absent_by_recorded_id_and_name_after_dind_failure()
-> Result<(), HostError> {
    let scratch = Scratch::open("retry-absent")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 2)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(204, ""),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(500, r#"{"message":"busy"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(204, ""),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };

    let first_result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(
        first_result,
        Err(Failure::request("container delete", HostError::Docker))
    );
    let retry_delay = cleanup_retry_delay_seconds(claim.attempt);
    if !journal
        .retry_completion_cleanup(launch.intent.id, claim.generation, retry_delay)
        .await?
    {
        return Err(HostError::Journal);
    }
    let retry_at = completion_now()?
        .checked_add(retry_delay)
        .ok_or(HostError::Journal)?;
    let retry_claim = journal
        .claim_completion_cleanup_at(launch.intent.id, retry_at, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let retry_result = run_cleanup(&context, &launch, retry_claim).await;
    assert_eq!(retry_result, Ok(()));

    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 18);
    assert!(requests[2].starts_with("GET /containers/runner-id/json"));
    assert!(requests[3].starts_with("DELETE "));
    assert!(requests[8].starts_with("GET /containers/dind-id/json"));
    assert!(requests[9].starts_with("DELETE "));
    assert!(requests[10].contains(RUNNER_ID));
    assert!(requests[11].contains("wcompletion-runner"));
    assert!(requests[15].starts_with("DELETE "));
    Ok(())
}

#[tokio::test]
async fn retry_rejects_a_different_container_at_the_durable_name() -> Result<(), HostError> {
    let scratch = Scratch::open("replacement")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json("replacement-id", "runner")),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };
    let result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(result, Err(Failure::not_proven("container name identity")));
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    Ok(())
}

#[tokio::test]
async fn diagnostic_capture_failure_keeps_container_and_capacity() -> Result<(), HostError> {
    let scratch = Scratch::open("capture-failure")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json("foreign-runner", "runner")),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };

    let result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(
        result,
        Err(Failure::request(
            "container diagnostic inspect",
            HostError::Ownership
        ))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    let diagnostic_scope = scratch
        .directory
        .join(SCOPE_DIRECTORY)
        .join(scope_component("root"));
    if std::fs::read_dir(diagnostic_scope)
        .map_err(|_| HostError::Path)?
        .next()
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}
