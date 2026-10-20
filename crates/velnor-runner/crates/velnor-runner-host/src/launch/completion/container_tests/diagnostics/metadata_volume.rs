use super::*;

#[tokio::test]
async fn metadata_volume_delete_failure_keeps_capacity_until_cleanup_proof() -> Result<(), HostError>
{
    let scratch = Scratch::open("metadata-volume-hold")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let worker = launch
        .intent
        .worker_volume
        .as_deref()
        .ok_or(HostError::Journal)?;
    let metadata_name = format!("{worker}-metadata");
    let metadata_role = crate::worker::WorkerVolumeRole::RunnerMetadata.label();
    let stub = DockerStub::open(vec![
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(200, &volume_json(&metadata_name, worker, metadata_role)),
        response(500, r#"{"message":"busy"}"#),
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
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    let result = volume::cleanup(&context, &launch, claim, &mut budget).await;
    assert_eq!(
        result,
        Err(Failure::request("volume delete", HostError::Docker))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 6);
    assert!(requests[4].contains(&metadata_name));
    assert!(requests[5].starts_with("DELETE "));
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}
