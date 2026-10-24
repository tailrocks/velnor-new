use super::*;

#[tokio::test]
async fn branch_restricted_pull_request_is_admitted_from_authenticated_api_records()
-> Result<(), String> {
    let encoded = host_config(
        &[basic_connection(
            "source-pr-admission",
            77,
            "chainargos/java-monorepo",
            "source-pr-admission",
            "integration",
        )],
        1,
    );
    let config = HostConfig::parse(&encoded).map_err(|error| error.to_string())?;
    let mut connection = config
        .connections()
        .first()
        .ok_or_else(|| "connection fixture missing".to_owned())?
        .clone();
    connection.source_policy =
        policy(vec![WorkflowEvent::PullRequest]).map_err(|error| error.to_string())?;

    let (_scratch, journal) = launch_harness::open("source-pr-admission").await?;
    let scope = journal
        .connection_scope(&connection)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_scale_set(501, connection.scale_set.name(), SetOwnership::Created)
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let source = SourceMetadata {
        owner_name: Some("chainargos".to_owned()),
        repository_name: Some("java-monorepo".to_owned()),
        job_workflow_ref: Some(
            "chainargos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge".to_owned(),
        ),
        workflow_run_id: Some(900),
        event_name: Some("pull_request".to_owned()),
    };
    let mut transport = SourceApiFixture {
        authorization: None,
        authorizations: Vec::new(),
        pull_request_responses: true,
        paths: Vec::new(),
    };

    let receipt = super::super::authorize_job_with_transport(
        &connection,
        &binding,
        &source,
        44,
        7,
        "connection-pat",
        &mut transport,
    )
    .map_err(|error| format!("PR admission blocked: {error:?}"))?;

    assert_eq!(receipt.request_id(), 44);
    assert_eq!(receipt.message_id(), 7);
    assert_eq!(receipt.policy_mode(), SourcePolicyMode::BranchRestricted);
    assert_eq!(receipt.source().event_name, "pull_request");
    assert_eq!(
        receipt.source().job_workflow.kind,
        WorkflowRefKind::PullRequest
    );
    assert_eq!(
        receipt.source().job_workflow.reference,
        "refs/pull/2110/merge"
    );
    assert_eq!(receipt.source().head_sha, "b".repeat(40));
    assert_eq!(receipt.source().workflow_source_sha, None);
    assert_api_calls(&transport);
    // This admission receipt contains no tested commit or execution authority;
    // the separate Started verifier must prove the actual merge commit later.
    Ok(())
}

fn assert_api_calls(transport: &SourceApiFixture) {
    assert_eq!(
        transport.paths,
        [
            "/repos/chainargos/java-monorepo/actions/runs/900",
            "/repos/chainargos/java-monorepo/pulls/2110",
        ]
    );
    assert_eq!(
        transport.authorizations,
        ["Bearer connection-pat", "Bearer connection-pat"]
    );
}
