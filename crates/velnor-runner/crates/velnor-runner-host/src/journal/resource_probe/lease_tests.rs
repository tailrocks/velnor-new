use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::HostError;
use crate::journal::{
    CapacityLimits, DoctorProbePrepareResult, DoctorProbeStartResult, DoctorProbeState, Journal,
    OperationalCapacityLimits, ProbeLeaseState, ProbePhase, ProbeSeed,
};
use crate::launch::VerifiedProbeCleanup;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-probe-lease-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn worker_limits() -> CapacityLimits {
    CapacityLimits {
        jobs: 1,
        cpu_millicores: 2_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
    }
}

fn operational_limits(cpu_millicores: u64, memory_bytes: u64) -> OperationalCapacityLimits {
    OperationalCapacityLimits {
        cpu_millicores,
        memory_bytes,
    }
}

async fn setup(journal: &Journal, cpu: u64, memory: u64) -> Result<(), HostError> {
    journal.bind_engine("probe-engine").await?;
    journal
        .configure_capacity_with_operational_limits(
            worker_limits(),
            operational_limits(cpu, memory),
        )
        .await
}

async fn seed(journal: &Journal, id: &str) -> Result<ProbeSeed, HostError> {
    let instance_id = journal.instance_id().await?;
    Ok(ProbeSeed {
        operation_id: id.repeat(32),
        instance_id,
        engine_id: "probe-engine".to_owned(),
        docker_root_digest: "a".repeat(64),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: format!("velnor-resource-probe-{}", id.repeat(32)),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: "5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf"
            .to_owned(),
    })
}

#[tokio::test]
async fn lease_is_charged_before_create_and_unknown_absence_keeps_it_after_reopen()
-> Result<(), HostError> {
    let scratch = Scratch::new();
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    setup(&journal, 100, 128 * 1024 * 1024).await?;
    let first = seed(&journal, "a").await?;
    journal.prepare_probe(first).await?;
    assert_eq!(
        journal.active_probe().await?.map(|row| row.lease_state),
        Some(ProbeLeaseState::Reserved)
    );
    journal
        .transition_probe(
            &"a".repeat(32),
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let active = reopened.active_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(active.phase, ProbePhase::CreateRequested);
    assert_eq!(active.lease_state, ProbeLeaseState::Uncertain);
    assert!(!reopened.is_drained().await?);
    assert!(
        reopened
            .begin_probe_recovery(&active.operation_id)
            .await
            .is_ok()
    );
    assert_eq!(
        reopened.active_probe().await?.map(|row| row.lease_state),
        Some(ProbeLeaseState::Recovering)
    );
    assert!(
        reopened
            .transition_probe(
                &active.operation_id,
                ProbePhase::CreateRequested,
                ProbePhase::Removed,
            )
            .await
            .is_err()
    );
    assert!(
        reopened
            .prepare_probe(seed(&reopened, "f").await?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn probe_charge_requires_the_persisted_operational_limit() -> Result<(), HostError> {
    let scratch = Scratch::new();
    let journal = Journal::open(&scratch.file()).await?;
    setup(&journal, 99, 128 * 1024 * 1024).await?;
    assert!(
        journal
            .prepare_probe(seed(&journal, "b").await?)
            .await
            .is_err()
    );
    assert!(journal.active_probe().await?.is_none());
    journal
        .configure_capacity_with_operational_limits(
            worker_limits(),
            operational_limits(100, 128 * 1024 * 1024),
        )
        .await?;
    journal.prepare_probe(seed(&journal, "c").await?).await?;
    Ok(())
}

#[tokio::test]
async fn only_exact_cleanup_proof_retires_probe_and_lease_together() -> Result<(), HostError> {
    let scratch = Scratch::new();
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    setup(&journal, 100, 128 * 1024 * 1024).await?;
    let operation_id = "c".repeat(32);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    journal.prepare_probe(seed(&journal, "c").await?).await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let container_id = "f".repeat(64);
    journal
        .bind_probe_container(&operation_id, &container_id)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;

    let invalid = VerifiedProbeCleanup::test_only(
        operation_id.clone(),
        "other-engine".to_owned(),
        "a".repeat(64),
        ProbePhase::RemoveRequested,
        operation_name.clone(),
        Some(container_id.clone()),
    );
    assert!(journal.retire_probe(invalid).await.is_err());
    assert_eq!(
        journal.active_probe().await?.map(|row| row.lease_state),
        Some(ProbeLeaseState::Uncertain)
    );

    let exact = VerifiedProbeCleanup::test_only(
        operation_id.clone(),
        "probe-engine".to_owned(),
        "a".repeat(64),
        ProbePhase::RemoveRequested,
        operation_name,
        Some(container_id),
    );
    journal.retire_probe(exact).await?;
    assert!(journal.active_probe().await?.is_none());
    assert!(journal.is_drained().await?);

    drop(journal);
    let reopened = Journal::open(&path).await?;
    assert!(reopened.is_drained().await?);
    reopened.prepare_probe(seed(&reopened, "d").await?).await?;
    assert_eq!(
        reopened.active_probe().await?.map(|row| row.lease_state),
        Some(ProbeLeaseState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn queued_doctor_does_not_hide_generic_cleanup_or_allow_new_generic_probe()
-> Result<(), HostError> {
    let scratch = Scratch::new();
    let journal = Journal::open(&scratch.file()).await?;
    setup(&journal, 100, 128 * 1024 * 1024).await?;

    let generic = seed(&journal, "a").await?;
    journal.prepare_probe(generic.clone()).await?;
    let ticket = match journal
        .start_doctor_probe_request(&"1".repeat(32), "probe-engine", &"a".repeat(64))
        .await?
    {
        DoctorProbeStartResult::Created(ticket) => ticket,
        DoctorProbeStartResult::Replayed(_) | DoctorProbeStartResult::Busy { .. } => {
            return Err(HostError::Journal);
        }
    };

    assert_eq!(
        journal.active_probe().await?.map(|row| row.operation_id),
        Some(generic.operation_id.clone())
    );
    assert!(
        journal
            .prepare_probe(seed(&journal, "b").await?)
            .await
            .is_err()
    );

    journal
        .transition_probe(
            &generic.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let container_id = "f".repeat(64);
    journal
        .bind_probe_container(&generic.operation_id, &container_id)
        .await?;
    journal
        .transition_probe(
            &generic.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            generic.operation_id.clone(),
            "probe-engine".to_owned(),
            "a".repeat(64),
            ProbePhase::RemoveRequested,
            generic.operation_name,
            Some(container_id),
        ))
        .await?;

    assert!(journal.active_probe().await?.is_none());
    assert!(
        journal
            .prepare_probe(seed(&journal, "c").await?)
            .await
            .is_err()
    );
    let status = journal
        .doctor_probe_request_by_nonce(&"1".repeat(32))
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(status.state(), DoctorProbeState::Queued);

    let doctor_child = seed(&journal, "d").await?;
    let prepared = journal
        .prepare_probe_for_doctor_request(&ticket, doctor_child.clone())
        .await?;
    let DoctorProbePrepareResult::Prepared(row) = prepared else {
        return Err(HostError::Journal);
    };
    assert_eq!(row.operation_id, doctor_child.operation_id);
    assert!(journal.active_probe().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn version_eleven_migration_backfills_an_uncertain_probe_lease() -> Result<(), HostError> {
    let scratch = Scratch::new();
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    setup(&journal, 1_000, 256 * 1024 * 1024).await?;
    journal.prepare_probe(seed(&journal, "d").await?).await?;
    journal
        .transition_probe(
            &"d".repeat(32),
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for trigger in [
        "doctor_probe_link_guard",
        "doctor_probe_transition_guard",
        "doctor_probe_terminal_guard",
        "doctor_probe_no_delete",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP TABLE doctor_probe_requests", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER metadata_settlement_claim_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER metadata_probe_cleanup_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE host_operation_leases", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE host_capacity_policy DROP COLUMN operational_cpu_millicores",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE host_capacity_policy DROP COLUMN operational_memory_bytes",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 11", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let migrated = Journal::open(&path).await?;
    let row = migrated.active_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::CreateRequested);
    assert_eq!(row.lease_state, ProbeLeaseState::Uncertain);
    assert!(
        migrated
            .prepare_probe(seed(&migrated, "e").await?)
            .await
            .is_err()
    );
    Ok(())
}
