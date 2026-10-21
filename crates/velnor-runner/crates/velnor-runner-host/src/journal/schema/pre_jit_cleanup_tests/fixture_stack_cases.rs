use super::*;

pub(super) async fn claimed_pre_jit_recovery_accepts_progress_without_changing_reservation_identity_on_fixture_stack()
-> Result<(), String> {
    let failure = crate::launch::runner_create_409().await?;
    let reservation_id = failure.reservation_id();
    let scope = failure.scope().clone();
    let claim = scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not claim pre-JIT cleanup.".to_owned())?
        .ok_or_else(|| "The terminal Runner create did not permit cleanup.".to_owned())?;

    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        let updated = connection
            .execute(
                "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = 'bootstrap_absent', evidence_sha256 = ?1 WHERE reservation_id = ?2 AND claim_generation = ?3 AND phase = 'claimed'",
                (claim.snapshot_sha256(), reservation_id, claim.generation()),
            )
            .await
            .map_err(|_| crate::error::HostError::Journal)?;
        if updated != 1 {
            return Err(crate::error::HostError::Journal);
        }
        Ok(())
    })
    .await
    .map_err(|_| "The active claim did not accept phase-compatible source state.".to_owned())?;

    if !scope
        .validate_pre_jit_cleanup_claim(&claim)
        .await
        .map_err(|_| "The journal could not validate its progressed cleanup claim".to_owned())?
    {
        return Err("The active claim became invalid after cleanup progress.".to_owned());
    }
    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        for phase in ["initializer_absent", "storage_absent", "network_absent", "settled"] {
            let updated = connection
                .execute(
                    "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = ?1, evidence_sha256 = ?2 WHERE reservation_id = ?3 AND claim_generation = ?4",
                    (phase, claim.snapshot_sha256(), reservation_id, claim.generation()),
                )
                .await
                .map_err(|_| crate::error::HostError::Journal)?;
            if updated != 1 {
                return Err(crate::error::HostError::Journal);
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| "The progress record rejected its ordered completion stages".to_owned())?;
    if scope
        .validate_pre_jit_cleanup_claim(&claim)
        .await
        .map_err(|_| "The journal could not validate a terminal cleanup claim".to_owned())?
    {
        return Err("The settled claim remained available for more effects.".to_owned());
    }
    if scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not reject a settled cleanup claim".to_owned())?
        .is_some()
    {
        return Err("The journal reissued a settled cleanup claim".to_owned());
    }
    let _captured = failure.finish().await?;
    Ok(())
}

pub(super) async fn runner_create_terminal_reply_allows_one_live_pre_jit_claim_on_fixture_stack()
-> Result<(), String> {
    let failure = crate::launch::runner_create_409().await?;
    if failure.error().is_empty() {
        return Err("The production runner create path returned no failure.".to_owned());
    }
    let reservation_id = failure.reservation_id();
    let scope = failure.scope().clone();
    let receipt = scope
        .bootstrap_container_create_terminal_response(
            reservation_id,
            crate::launch::bootstrap::BootstrapContainerRole::Runner,
        )
        .await
        .map_err(|_| "The journal could not read the Runner create response.".to_owned())?
        .ok_or_else(|| "The journal did not retain the Runner create response.".to_owned())?;
    assert_eq!(receipt.status_code(), 409);
    assert_eq!(receipt.client_api_version(), (1, 53));

    let claim = scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not claim pre-JIT cleanup.".to_owned())?
        .ok_or_else(|| "The terminal Runner create did not permit cleanup.".to_owned())?;
    assert_eq!(claim.operation_id(), receipt.operation_id());
    assert_eq!(claim.engine_id(), receipt.engine_id());
    assert!(claim.lease_until() > 0);
    assert!(
        scope
            .validate_pre_jit_cleanup_claim(&claim)
            .await
            .map_err(|_| "The journal could not validate its cleanup claim.".to_owned())?
    );
    assert!(
        scope
            .claim_pre_jit_cleanup(reservation_id)
            .await
            .map_err(|_| "The journal could not check a competing cleanup claim.".to_owned())?
            .is_none()
    );

    let captured = failure.finish().await?;
    if captured.server_error.is_some()
        || captured
            .requests
            .iter()
            .filter(|request| {
                request.target.contains("/containers/create")
                    && request.response_status == Some(409)
            })
            .count()
            != 1
        || !captured.jit_input.is_empty()
    {
        return Err(format!(
            "The test server did not record one Runner 409 with JIT withheld: error={:?}, creates={:?}, jit_bytes={}",
            captured.server_error,
            captured
                .requests
                .iter()
                .filter(|request| request.target.contains("/containers/create"))
                .map(|request| (&request.target, request.response_status))
                .collect::<Vec<_>>(),
            captured.jit_input.len(),
        ));
    }

    Ok(())
}
