// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::controller::Controller;
use super::{CRASHED, Crash, Prepared, TestResult, bootstrap_seed, db, prepare};

async fn rejects_unknown_absence(crash: Crash) -> TestResult<()> {
    let case = prepare(crash, false).await?;
    match crash {
        Crash::DindCreate => {
            case.fixture.remove_container(&CRASHED.container_id("dind"));
        }
        Crash::VolumeCreate => {
            case.fixture.remove_volume(&CRASHED.volumes()[3].0);
        }
        Crash::NetworkCreate => {
            case.fixture.remove_network(CRASHED);
        }
        _ => return Err("unknown creation family".into()),
    }
    let case = case.restart().await?;
    case.fixture.take_requests();
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    controller
        .turn(&case.journal, &case.docker, &case.engine)
        .await?;
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert!(
        db::generation(&case.journal).await? > 0,
        "unknown create needs an owning retirement claim"
    );
    let claim = case
        .scope
        .claim_unbound_retirement(CRASHED.reservation)
        .await?
        .ok_or("deferred cleanup must allow a fresh claim")?;
    case.scope.validate_unbound_retirement_claim(&claim).await?;
    reject_first_phase(&case).await?;
    assert_eq!(
        db::number(
            &case.journal,
            "SELECT COUNT(*) FROM physical_attempt_unbound_retirements WHERE reservation_id=2"
        )
        .await?,
        0
    );
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    db::no_dispatch(&case.fixture);
    Ok(())
}

async fn reject_first_phase(case: &Prepared) -> TestResult<()> {
    let connection = case.journal.connection().await?;
    let query = "SELECT COUNT(*) FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id=2";
    assert_eq!(db::number(&case.journal, query).await?, 0);
    let generation = db::generation(&case.journal).await?;
    let sql = format!(
        "INSERT INTO capacity_reservation_unbound_retirement_progress(reservation_id,phase,claim_generation,evidence_sha256) VALUES(2,'containers_absent',{generation},'{}')",
        "a".repeat(64)
    );
    assert!(
        connection.execute(&sql, ()).await.is_err(),
        "unresolved creation must block the first absence proof"
    );
    assert_eq!(db::number(&case.journal, query).await?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_container_receipt_cannot_insert_an_absence_phase() -> TestResult<()> {
    Box::pin(rejects_unknown_absence(Crash::DindCreate)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn unknown_volume_creation_blocks_absence_proofs() -> TestResult<()> {
    Box::pin(rejects_unknown_absence(Crash::VolumeCreate)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn unknown_network_creation_blocks_absence_proofs() -> TestResult<()> {
    Box::pin(rejects_unknown_absence(Crash::NetworkCreate)).await
}
