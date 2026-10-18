use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, CapacityLimits, ConnectionJournal, Journal, ReservationRequest,
    ReservationResult, StorageAdmission, StorageAdmissionSample, StorageMedium, StoragePoolLimit,
    StoragePoolObservation, StorageProfile, StorageReservationPlan, StorageRole, StorageRoleCharge,
    StorageRoleLimit, StorageSampleIdentity,
};
use crate::launch::StartPermit;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-storage-cache-projection-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
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

fn binding(id: &str, repository_id: i64, repository: &str) -> RepositoryConnection {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .expect("selector");
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .expect("policy");
    RepositoryConnection::try_new(
        ConnectionId::parse(id).expect("connection ID"),
        repository_id,
        repository.to_owned(),
        KeychainReference::parse(&format!("keychain:velnor/{id}")).expect("keychain ref"),
        selector,
        policy,
    )
    .expect("connection")
}

fn profile() -> StorageProfile {
    let pool_id = "c".repeat(64);
    let roles = StorageRole::ALL
        .into_iter()
        .map(|role| match role {
            StorageRole::CacheLive | StorageRole::CacheStage => StorageRoleLimit::new_alias(
                role,
                pool_id.clone(),
                StorageMedium::Disk,
                StorageRole::RunnerWork,
                128,
                2,
            ),
            StorageRole::RunnerWork => {
                StorageRoleLimit::new(role, pool_id.clone(), StorageMedium::Disk, 12_000, 200, 0)
            }
            _ => StorageRoleLimit::new(role, pool_id.clone(), StorageMedium::Disk, 16, 1, 0),
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("role limits");
    StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new(pool_id, StorageMedium::Disk, 16_384, 256, 128, 4).expect("pool"),
        ],
        roles,
    )
    .expect("profile")
}

fn admission(profile: &StorageProfile, permit: &StartPermit) -> StorageAdmission {
    let pool_id = "c".repeat(64);
    let filesystem_instance_digest = "d".repeat(64);
    let roles = profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none())
        .map(|role| {
            let (charge_bytes, charge_inodes) = if role.role() == StorageRole::RunnerWork {
                (1024, 16)
            } else {
                (1, 1)
            };
            StorageRoleCharge::new(
                role.role(),
                pool_id.clone(),
                filesystem_instance_digest.clone(),
                StorageMedium::Disk,
                charge_bytes,
                charge_inodes,
                0,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("role charges");
    let plan = StorageReservationPlan::new(
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        "1".repeat(64),
        roles,
    )
    .expect("plan");
    let identity = StorageSampleIdentity::new(
        permit.sample_generation_id().to_owned(),
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        permit.engine_id().to_owned(),
        permit.docker_root_digest().to_owned(),
    )
    .expect("sample identity");
    let sample = StorageAdmissionSample::new(
        identity,
        Instant::now(),
        vec![
            StoragePoolObservation::new(
                pool_id,
                filesystem_instance_digest,
                StorageMedium::Disk,
                32_768,
                32_768,
                512,
                512,
            )
            .expect("pool observation"),
        ],
        None,
    )
    .expect("sample");
    StorageAdmission::new(plan, sample).expect("admission")
}

async fn scoped(
    journal: &Journal,
    id: &str,
    repository_id: i64,
) -> Result<ConnectionJournal, HostError> {
    journal.bind_engine("test-engine").await?;
    let scope = journal
        .connection_scope(&binding(
            id,
            repository_id,
            "example/storage-cache-projection",
        ))
        .await?;
    scope
        .bind_scale_set(repository_id + 1000, "integration", SetOwnership::Created)
        .await?;
    scope
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    Ok(scope)
}

#[tokio::test]
async fn cache_projection_is_scoped_fresh_read_only_and_stops_after_effect_claim()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    let first = scoped(&journal, "essential", 101).await?;
    let second = scoped(&journal, "chainargos", 202).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 4000,
            memory_bytes: 16 * 1024 * 1024 * 1024,
        })
        .await?;
    let profile = profile();
    journal.configure_storage_profile(profile.clone()).await?;
    let reordered = StorageProfile::new(
        profile.generation_id().to_owned(),
        profile.pools().iter().cloned().rev().collect(),
        profile.roles().iter().cloned().rev().collect(),
    )?;
    assert_eq!(reordered, profile);
    journal.configure_storage_profile(reordered).await?;
    let assignment = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 77 })
        .await?;
    let permit = StartPermit::test_fixture();
    let root_digest = permit.docker_root_digest().to_owned();
    let sample_generation = permit.sample_generation_id().to_owned();
    let storage = admission(&profile, &permit);
    let request = ReservationRequest {
        assignment: &assignment,
        engine_id: "test-engine",
        docker_root_digest: &root_digest,
        cpu_millicores: 2000,
        memory_bytes: 8 * 1024 * 1024 * 1024,
        sample_generation_id: &sample_generation,
    };
    let reservation_id = match first
        .try_reserve_with_storage(request, permit, storage)
        .await?
    {
        ReservationResult::Reserved { reservation_id } => reservation_id,
        result => panic!("unexpected reservation result: {result:?}"),
    };

    let before_read = journal.revision().await?;
    let projection = first
        .storage_cache_projection(&assignment, reservation_id)
        .await?
        .expect("fresh projection");
    assert_eq!(journal.revision().await?, before_read);
    assert_eq!(projection.reservation_id(), reservation_id);
    assert!(projection.launch_intent_id() > 0);
    assert_eq!(projection.profile_generation_id(), "a".repeat(32));
    assert_eq!(projection.sample_generation_id(), sample_generation);
    assert_eq!(projection.engine_id(), "test-engine");
    assert_eq!(projection.docker_root_digest(), root_digest);
    assert_eq!(projection.pool_id(), "c".repeat(64));
    assert_eq!(projection.filesystem_instance_digest(), "d".repeat(64));
    assert_eq!(projection.cache_live_bytes(), 128);
    assert_eq!(projection.cache_live_inodes(), 2);
    assert_eq!(projection.cache_stage_bytes(), 128);
    assert_eq!(projection.cache_stage_inodes(), 2);

    let other_assignment = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 77 })
        .await?;
    assert!(
        second
            .storage_cache_projection(&other_assignment, reservation_id)
            .await?
            .is_none()
    );
    let mismatched_assignment = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 78 })
        .await?;
    assert!(
        first
            .storage_cache_projection(&mismatched_assignment, reservation_id)
            .await?
            .is_none()
    );

    first.mark_uncertain(reservation_id).await?;
    assert!(
        first
            .storage_cache_projection(&assignment, reservation_id)
            .await?
            .is_none()
    );
    Ok(())
}
