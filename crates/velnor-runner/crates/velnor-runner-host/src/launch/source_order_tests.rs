use crate::journal::{CreatedScaleSetResponse, ScaleSetCreateRequest};
use crate::launch_harness::{available, open};
use crate::scale_set::EnsureError;
use crate::worker::guest_image::test_support::TestGuestImage;

#[tokio::test]
async fn incomplete_source_is_rejected_by_active_offer_authorization() -> Result<(), String> {
    let (_scratch, journal) = open("source-before-acquire").await?;
    let fixture = TestGuestImage::new().map_err(|error| error.to_string())?;
    let scope = journal
        .connection_scope(&fixture.connection)
        .await
        .map_err(|error| error.to_string())?;
    record_scale_set(&scope, &fixture.connection, 501).await?;
    let polled = available(&[4]);

    let result = crate::launch::authorize_positive_offer(
        &fixture.connection,
        &scope,
        "test-pat",
        501,
        &polled,
    )
    .await;
    let Err(error) = result else {
        return Err(
            "The source authorization code accepted a request without source data.".to_owned(),
        );
    };
    assert!(
        matches!(
            error,
            EnsureError::SourceBlocked {
                reason: "source_metadata_missing"
            }
        ),
        "Admission returned an unexpected error. Error: {error:?}"
    );
    assert_eq!(
        scope
            .physical_runner_reservations()
            .await
            .map_err(|error| error.to_string())?,
        Vec::<crate::journal::PhysicalRunnerReservation>::new()
    );
    Ok(())
}

async fn record_scale_set(
    scope: &crate::journal::ConnectionJournal,
    connection: &crate::config::RepositoryConnection,
    scale_set_id: i64,
) -> Result<(), String> {
    let request = ScaleSetCreateRequest::new(connection, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&request)
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The Scale Set creation claim is missing.".to_owned())?;
    let response = CreatedScaleSetResponse::verified(
        &request,
        scale_set_id,
        "linux-build",
        connection.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(claim, response)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}
