// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CRASHED, Crash, Journal, Prepared, TestResult, bootstrap_seed};

pub(super) async fn number(journal: &Journal, sql: &str) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(sql, ()).await?;
    Ok(rows.next().await?.ok_or("numeric journal row")?.get(0)?)
}

pub(super) async fn assert_current(journal: &Journal) -> TestResult<()> {
    assert_eq!(
        number(journal, "PRAGMA user_version").await?,
        Journal::current_schema_version_for_test()
    );
    Ok(())
}

pub(super) async fn assert_historical(journal: &Journal, crash: Crash) -> TestResult<()> {
    assert_eq!(number(journal, "PRAGMA user_version").await?, 48);
    let query = match crash {
        Crash::DindCreate => {
            "SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers WHERE reservation_id=2 AND role='dind' AND create_state='requested' AND container_id IS NULL"
        }
        Crash::RunnerCreate => {
            "SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers WHERE reservation_id=2 AND role='runner' AND create_state='requested' AND container_id IS NULL"
        }
        Crash::Start => {
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_state WHERE reservation_id=2 AND bootstrap_start_state='requested'"
        }
        Crash::VolumeCreate => {
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id=2 AND role='dind-data' AND create_state='requested' AND docker_id IS NULL"
        }
        Crash::NetworkCreate => {
            "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND create_state='requested' AND network_id IS NULL"
        }
        Crash::MetadataRecorded => {
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id=2 AND role='engine-metadata' AND create_state='created' AND docker_id=docker_name"
        }
        Crash::NetworkRecorded => {
            "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND create_state='created' AND network_id IS NOT NULL"
        }
        Crash::InitializerUnfinished => {
            "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE reservation_id=2 AND result_state='not_requested' AND remove_state='not_requested'"
        }
    };
    assert_eq!(number(journal, query).await?, 1);
    Ok(())
}

pub(super) async fn generation(journal: &Journal) -> TestResult<i64> {
    number(journal, "SELECT COALESCE(MAX(claim_generation),0) FROM capacity_reservation_unbound_retirement_claims WHERE reservation_id=2").await
}

pub(super) async fn unresolved(journal: &Journal, network: bool) -> TestResult<i64> {
    assert!(bootstrap_seed::charged(journal, CRASHED.reservation).await?);
    assert_eq!(
        number(
            journal,
            "SELECT COUNT(*) FROM physical_attempt_unbound_retirements WHERE reservation_id=2"
        )
        .await?,
        0
    );
    let query = if network {
        "SELECT COUNT(*) FROM capacity_reservation_networks WHERE reservation_id=2 AND remove_state='removed'"
    } else {
        "SELECT COUNT(*) FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id=2 AND phase='containers_absent'"
    };
    assert_eq!(number(journal, query).await?, 0);
    let generation = generation(journal).await?;
    assert!(
        generation > 0,
        "restart must own the uncertain create before deferring its 404"
    );
    let target = if network {
        "SELECT COUNT(*) FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=2 AND resource_kind='network' AND role='private'"
    } else {
        "SELECT COUNT(*) FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=2 AND resource_kind='container' AND role='dind'"
    };
    assert_eq!(
        number(journal, target).await?,
        0,
        "a 404 cannot mint a positive creation target"
    );
    Ok(generation)
}

pub(super) async fn target(
    journal: &Journal,
    kind: &str,
    role: &str,
    name: &str,
    id: &str,
) -> TestResult<()> {
    assert_eq!(number(journal, "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='capacity_reservation_unbound_creation_targets'").await?, 1, "DELETE occurred before a durable exact target ledger existed");
    let connection = journal.connection().await?;
    let mut rows = connection.query(
        "SELECT resource_kind,role,docker_name,docker_id,observation_sha256 FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=2 AND resource_kind=?1 AND role=?2",
        (kind, role),
    ).await?;
    let row = rows
        .next()
        .await?
        .ok_or("exact target must be durable before DELETE")?;
    assert_eq!(row.get::<String>(0)?, kind);
    assert_eq!(row.get::<String>(1)?, role);
    assert_eq!(row.get::<String>(2)?, name);
    assert_eq!(row.get::<String>(3)?, id);
    let digest = row.get::<String>(4)?;
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(rows.next().await?.is_none());
    Ok(())
}

pub(super) async fn released(
    case: &Prepared,
    neighbor: &str,
    objects: &[String],
) -> TestResult<()> {
    assert!(!bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert_eq!(case.journal.readiness_capacity_usage().await?.jobs, 1);
    assert_eq!(
        super::super::seed::cleanup_proven(&case.journal, CRASHED.reservation).await?,
        (1, 1, 1)
    );
    assert_eq!(case.fixture.objects(CRASHED), Vec::<String>::new());
    assert!(!case.fixture.has_initializer(CRASHED));
    assert_eq!(
        bootstrap_seed::state(&case.journal, super::NEIGHBOR.reservation).await?,
        neighbor
    );
    assert_eq!(case.fixture.objects(super::NEIGHBOR), objects);
    bootstrap_seed::assert_planning(&case.scope, CRASHED.reservation, &case.policy, false).await?;
    no_dispatch(&case.fixture);
    Ok(())
}

pub(super) fn no_dispatch(fixture: &super::FixtureDocker) {
    assert!(
        !fixture
            .take_requests()
            .iter()
            .any(|request| request.contains("/create") || request.ends_with("/start"))
    );
}
