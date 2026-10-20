use super::*;

#[tokio::test]
async fn foreign_volume_owner_prevents_cleanup_proof_and_keeps_capacity() -> Result<(), HostError> {
    let scratch = Scratch::open("foreign-volume-owner")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let role = crate::worker::WorkerVolumeRole::ALL
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    let worker = launch
        .intent
        .worker_volume
        .as_deref()
        .ok_or(HostError::Journal)?;
    let identity = crate::worker::WorkerIdentityNames::from_private_volume(worker)?;
    let name = identity.volume_name(role);
    let stub = DockerStub::open(vec![response(
        200,
        &volume_json(&name, "another-worker", role.label()),
    )])?;
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

    assert_eq!(
        volume::cleanup(&context, &launch, claim, &mut budget).await,
        Err(Failure::not_proven("volume ownership"))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET "));
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}
