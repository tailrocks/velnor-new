// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use crate::journal::{Journal, UnboundRetirementPhase};

use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, RETIRED, TestResult, retire_unbound_pair};

const CLAIM: &str = "SELECT claim_generation || '/' || lease_until FROM capacity_reservation_unbound_retirement_claims WHERE reservation_id = ?1";

fn parts(text: &str) -> TestResult<(i64, i64)> {
    let (generation, lease) = text.split_once('/').ok_or("no claim row")?;
    Ok((generation.parse()?, lease.parse()?))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renewed_claim_extends_its_lease_and_still_records_progress() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, _engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    let claim = scope
        .claim_unbound_retirement(FAILED.reservation)
        .await?
        .ok_or("the pair was not claimable")?;
    let (generation, lease) = parts(&seed::text(&journal, CLAIM, FAILED.reservation).await?)?;

    tokio::time::sleep(Duration::from_millis(1100)).await;
    scope
        .renew_unbound_retirement_claim(&claim)
        .await
        .map_err(|e| format!("renew: {e:?}"))?;

    let (renewed_generation, renewed_lease) =
        parts(&seed::text(&journal, CLAIM, FAILED.reservation).await?)?;
    assert_eq!(renewed_generation, generation + 1);
    assert!(renewed_lease > lease);
    scope
        .validate_unbound_retirement_claim(&claim)
        .await
        .map_err(|e| format!("validate: {e:?}"))?;
    for role in ["runner", "dind"] {
        let name = format!("w{}-{role}", FAILED.nonce());
        scope
            .record_unbound_container_absent(&claim, role, &name)
            .await?;
    }
    scope
        .record_unbound_retirement_phase(
            &claim,
            UnboundRetirementPhase::ContainersAbsent,
            &"e".repeat(64),
        )
        .await
        .map_err(|e| format!("record: {e:?}"))?;
    assert!(
        scope
            .claim_unbound_retirement(FAILED.reservation)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_network_removal_left_uncertain_is_recorded_removed_on_the_retry() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    for state in ["requested", "uncertain"] {
        seed::execute(
            &journal,
            &format!(
                "UPDATE capacity_reservation_networks SET remove_state = '{state}' WHERE reservation_id = {}",
                FAILED.reservation
            ),
        )
        .await?;
    }

    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);

    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    Ok(())
}
