use super::*;

#[tokio::test]
async fn non_not_found_volume_absence_keeps_capacity_until_cleanup_proof() -> Result<(), HostError>
{
    let volume_count = crate::worker::WorkerVolumeRole::ALL.len();
    for failed_volume_index in [3, volume_count - 1] {
        let scratch = Scratch::open("volume-post-delete")?;
        let journal = Journal::open(&scratch.path).await?;
        let launch = completed_launch(&journal).await?;
        let completion_journal = CompletionJournal::root(journal.clone());
        let claim = journal
            .claim_completion_cleanup(launch.intent.id, 90)
            .await?
            .ok_or(HostError::Journal)?;
        let identity = crate::worker::WorkerIdentityNames::from_private_volume(WORKER)?;
        let mut responses = Vec::new();
        for (index, role) in crate::worker::WorkerVolumeRole::ALL
            .into_iter()
            .enumerate()
            .take(failed_volume_index + 1)
        {
            let name = identity.volume_name(role);
            let role = role.label();
            if index == failed_volume_index {
                responses.push(response(200, &volume_json(&name, WORKER, role)));
                responses.push(response(204, ""));
                responses.push(response(500, r#"{"message":"not absent"}"#));
            } else {
                responses.push(response(404, r#"{"message":"missing"}"#));
            }
        }
        let stub = DockerStub::open(responses)?;
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
            Err(Failure::request("volume absence verify", HostError::Docker))
        );
        let requests = stub.finish().await?;
        assert_eq!(requests.len(), failed_volume_index + 3);
        assert!(requests[failed_volume_index].starts_with("GET "));
        assert!(requests[failed_volume_index + 1].starts_with("DELETE "));
        assert!(requests[failed_volume_index + 2].starts_with("GET "));
        let rows = journal.rows().await?;
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].cleanup_proven);
        assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    }
    Ok(())
}
