pub(crate) struct ListenerDeliveryFixture {
    pub(crate) _scratch: Scratch,
    pub(crate) scope: ConnectionJournal,
    pub(crate) claim: crate::journal::ListenerDeliveryClaim,
    pub(crate) runner: velnor_runner_github::RunnerReference,
}

pub(crate) async fn listener_delivery_fixture(
    nonce: char,
) -> Result<ListenerDeliveryFixture, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    let repository = binding("listener-fixture", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let initialized =
        provision_acquired_request_with_initialization(&scope, 70, nonce, nonce).await?;
    let runner = velnor_runner_github::RunnerReference {
        id: 770,
        name: runner_name(nonce),
        runner_scale_set_id: 501,
    };
    let claim =
        prepare_initialized_listener_delivery(&scope, &repository, &initialized, &runner, 70)
            .await?;
    Ok(ListenerDeliveryFixture {
        _scratch: scratch,
        scope,
        claim,
        runner,
    })
}

