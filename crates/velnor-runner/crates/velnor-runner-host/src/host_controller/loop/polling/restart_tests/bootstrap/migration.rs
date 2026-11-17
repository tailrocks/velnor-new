// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::NetworkCreateRequest;

use super::super::controller::Controller;
use super::{CRASHED, Crash, NEIGHBOR, Prepared, TestResult, bootstrap_seed, db, prepare};
use crate::launch::bootstrap::historical_v48;

async fn historical() -> TestResult<(Prepared, crate::journal::UnboundRetirementClaim)> {
    let case = prepare(Crash::NetworkCreate, false).await?;
    super::seed::unguarded(&case.journal, &["UPDATE capacity_reservation_storage_initialization SET remove_state='removed',abort_state='absent',abort_lease_until=0 WHERE reservation_id=2".to_owned()]).await?;
    case.fixture.remove_container(&CRASHED.helper_id());
    case.fixture.remove_network(CRASHED);
    let claim = historical_v48::retire_before_settlement(
        &case.docker,
        &case.scope,
        &case.engine,
        CRASHED.reservation,
    )
    .await
    .map_err(|e| format!("historical retirement protocol: {e}"))?;
    assert_eq!(db::number(&case.journal, "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND network_id IS NULL AND remove_state='removed'").await?, 1);
    Ok((case, claim))
}

async fn proofs(case: &Prepared) -> TestResult<Vec<String>> {
    snapshot(
        case,
        &[
            "capacity_reservation_unbound_retirement_progress",
            "physical_attempt_unbound_retirements",
        ],
    )
    .await
}

async fn terminal_history(case: &Prepared) -> TestResult<Vec<String>> {
    assert!(!bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(
        db::number(
            &case.journal,
            "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=2"
        )
        .await?,
        0
    );
    snapshot(
        case,
        &[
            "capacity_reservation_unbound_retirement_claims",
            "capacity_reservation_networks",
            "capacity_reservation_unbound_retirement_progress",
            "physical_attempt_unbound_retirements",
        ],
    )
    .await
}

async fn snapshot(case: &Prepared, tables: &[&str]) -> TestResult<Vec<String>> {
    let connection = case.journal.connection().await?;
    let mut result = Vec::new();
    for table in tables {
        let mut rows = connection
            .query(
                &format!("SELECT * FROM {table} WHERE reservation_id=2 ORDER BY 1,2"),
                (),
            )
            .await?;
        while let Some(row) = rows.next().await? {
            let values = (0..row.column_count())
                .map(|index| row.get_value(index).map(|value| format!("{value:?}")))
                .collect::<Result<Vec<_>, _>>()?;
            result.push(format!("{table}: {values:?}"));
        }
    }
    Ok(result)
}

async fn submit_network(case: &Prepared) -> TestResult<()> {
    let connection = case.journal.connection().await?;
    let mut rows = connection.query("SELECT network_name,labels_json FROM capacity_reservation_networks WHERE reservation_id=2", ()).await?;
    let row = rows.next().await?.ok_or("network")?;
    case.docker
        .create_network(NetworkCreateRequest {
            name: row.get(0)?,
            labels: Some(serde_json::from_str(&row.get::<String>(1)?)?),
            driver: Some("bridge".to_owned()),
            ..Default::default()
        })
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn historical_removed_network_stays_charged_until_late_creation_is_deleted() -> TestResult<()>
{
    let (case, _claim) = historical().await?;
    let before = proofs(&case).await?;
    let name = format!("velnor-v{}-private", CRASHED.nonce());
    case.fixture.remove_network(CRASHED);
    let publication = case
        .fixture
        .delay_create("networks", &name, &CRASHED.network_id());
    let mut original = Box::pin(submit_network(&case));
    tokio::select! {
        result = &mut original => return Err(format!("create returned before acceptance: {result:?}").into()),
        _ = publication.accepted() => {},
    }
    drop(original);
    let case = case
        .restart()
        .await
        .map_err(|e| format!("migrating active historical network: {e}"))?;
    assert_eq!(proofs(&case).await?, before);
    assert_eq!(db::number(&case.journal, "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND network_id IS NULL AND remove_state='uncertain'").await?, 1);
    let neighbor = bootstrap_seed::state(&case.journal, NEIGHBOR.reservation).await?;
    let objects = case.fixture.objects(NEIGHBOR);
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    db::unresolved(&case.journal, true).await?;
    assert_eq!(proofs(&case).await?, before);
    publication.release();
    publication.published().await;
    let deletion = case.fixture.delay_delete("networks", &CRASHED.network_id());
    let mut cleanup = Box::pin(controller.turn(&case.journal, &case.docker, &case.engine));
    tokio::select! {
        result = &mut cleanup => return Err(format!("cleanup returned before exact DELETE: {result:?}").into()),
        () = deletion.reached() => {},
    }
    db::target(
        &case.journal,
        "network",
        "private",
        &name,
        &CRASHED.network_id(),
    )
    .await?;
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(
        db::number(
            &case.journal,
            "SELECT COUNT(*) FROM physical_attempt_unbound_retirements WHERE reservation_id=2"
        )
        .await?,
        0
    );
    drop(cleanup);
    drop(controller);
    deletion.release();
    let case = case.restart().await?;
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    db::released(&case, &neighbor, &objects).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn historical_settled_null_network_reopens_with_unchanged_terminal_proof() -> TestResult<()> {
    let (case, claim) = historical().await?;
    historical_v48::settle(&case.scope, &claim)
        .await
        .map_err(|e| format!("historical settlement: {e}"))?;
    let before = terminal_history(&case).await?;
    let state = db::creation_state(&case.journal).await?;
    let case = case
        .reopen()
        .await
        .map_err(|e| format!("migrating settled historical network: {e}"))?;
    assert_eq!(terminal_history(&case).await?, before);
    assert_eq!(db::creation_state(&case.journal).await?, state);
    assert!(!bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(db::number(&case.journal, "SELECT COUNT(*) FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=2").await?, 0);
    let connection = case.journal.connection().await?;
    for sql in [
        "INSERT OR REPLACE INTO capacity_reservation_unbound_historical_network_removals SELECT * FROM capacity_reservation_unbound_historical_network_removals",
        "UPDATE capacity_reservation_unbound_historical_network_removals SET network_name=network_name",
        "DELETE FROM capacity_reservation_unbound_historical_network_removals",
        "UPDATE capacity_reservation_networks SET remove_state='uncertain' WHERE reservation_id=2",
        "UPDATE capacity_reservation_networks SET network_name='borrowed' WHERE reservation_id=2",
    ] {
        assert!(
            connection.execute(sql, ()).await.is_err(),
            "history mutation accepted: {sql}"
        );
    }
    drop(connection);
    let case = case.reopen().await?;
    assert_eq!(terminal_history(&case).await?, before);
    Ok(())
}
