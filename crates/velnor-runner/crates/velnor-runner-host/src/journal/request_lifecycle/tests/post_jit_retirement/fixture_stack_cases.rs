use super::*;

pub(super) async fn retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails_on_fixture_stack()
-> Result<(), HostError> {
    Box::pin(
        retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails_inner(),
    )
    .await
}

pub(super) async fn retirement_settlement_writes_terminal_before_release_and_reopens_on_fixture_stack()
-> Result<(), HostError> {
    Box::pin(retirement_settlement_writes_terminal_before_release_and_reopens_inner()).await
}

async fn retirement_settlement_writes_terminal_before_release_and_reopens_inner()
-> Result<(), HostError> {
    let fixture = prepared_fixture().await?;
    seed_mocked_external_evidence(&fixture).await?;
    record_storage_and_network_proofs(&fixture).await?;
    let claim =
        helpers::create_cleaned_settlement_probe(&fixture.scope, &fixture.provisioned, 'a').await?;
    let sample = helpers::settlement_sample(&fixture.scope, &claim).await?;
    let outcome = fixture
        .scope
        .record_storage_metadata_settlement(&claim, sample)
        .await?;
    assert_eq!(outcome, MetadataSettlementResult::Settled);
    assert_terminal_state(&fixture).await?;
    let reopened = Journal::open(&fixture.database_path).await?;
    assert_reopened_terminal_state(&reopened, &fixture).await
}

async fn retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails_inner()
-> Result<(), HostError> {
    let fixture = prepared_fixture().await?;
    seed_mocked_external_evidence(&fixture).await?;
    record_storage_and_network_proofs(&fixture).await?;
    let claim =
        helpers::create_cleaned_settlement_probe(&fixture.scope, &fixture.provisioned, 'b').await?;
    let sample = helpers::settlement_sample(&fixture.scope, &claim).await?;
    install_current_delete_failure(&fixture).await?;
    assert!(
        fixture
            .scope
            .record_storage_metadata_settlement(&claim, sample)
            .await
            .is_err()
    );
    drop_current_delete_failure(&fixture).await?;
    assert_held_without_terminal(&fixture).await?;
    Ok(())
}
