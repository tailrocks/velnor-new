use super::*;

#[tokio::test]
async fn volume_delete_transport_failure_keeps_the_reservation_charged() -> Result<(), String> {
    assert_volume_failure_keeps_charge(false).await
}

#[tokio::test]
async fn post_delete_volume_inspection_failure_keeps_the_reservation_charged() -> Result<(), String>
{
    assert_volume_failure_keeps_charge(true).await
}

async fn assert_volume_failure_keeps_charge(inspect_failure: bool) -> Result<(), String> {
    let failure = crate::launch::steps::runner_create_409().await?;
    failure.set_probe_available_bytes(35 * 1024 * 1024 * 1024);
    let mut names = failure.storage_volume_names().await?;
    names.sort_unstable();
    let name = names
        .first()
        .ok_or_else(|| "The acquired reservation has no storage volume.".to_owned())?;
    if inspect_failure {
        failure.inject_volume_inspect_transport_failure(name);
    } else {
        failure.inject_volume_delete_transport_failure(name);
    }
    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    let result = crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        failure.scope(),
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await;
    if result.is_ok() {
        return Err("The injected storage cleanup failure did not stop cleanup.".to_owned());
    }
    assert_reservation_remains_charged(&failure).await?;
    let captured = failure.finish().await?;
    if !captured
        .server_error
        .as_deref()
        .is_some_and(|error| error.contains("transport failure"))
    {
        return Err(format!(
            "The Docker fixture did not surface the injected transport failure: {:?}.",
            captured.server_error
        ));
    }
    let delete = captured
        .requests
        .iter()
        .find(|request| request.method == "DELETE" && request.target == format!("/volumes/{name}"))
        .ok_or_else(|| "The selected volume was not sent for deletion.".to_owned())?;
    if inspect_failure && delete.response_status != Some(204) {
        return Err("Deletion did not succeed before the inspection failed.".to_owned());
    }
    if !inspect_failure && delete.response_status.is_some() {
        return Err("The delete transport failure returned a Docker response.".to_owned());
    }
    if inspect_failure
        && !captured.requests.iter().any(|request| {
            request.method == "GET"
                && request.target == format!("/volumes/{name}")
                && request.response_status.is_none()
        })
    {
        return Err("Post-delete inspection did not fail at transport layer.".to_owned());
    }
    Ok(())
}

async fn assert_reservation_remains_charged(
    failure: &crate::launch::steps::RunnerCreateFailure,
) -> Result<(), String> {
    assert_eq!(
        failure.host_journal().occupied_reservation_count().await,
        Ok(1),
        "An unverified volume absence must keep its reservation charged."
    );
    let reservation = failure
        .scope()
        .reservation(failure.assignment())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The held reservation record is absent.".to_owned())?;
    if reservation.state == crate::journal::ReservationState::Released
        || failure
            .scope()
            .current_physical_attempt(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err("The unverified volume cleanup released its physical attempt.".to_owned());
    }
    Ok(())
}
