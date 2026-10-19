#[derive(Clone)]
pub(super) struct ProvisionedRunner {
    pub(super) assignment: crate::journal::AssignmentIdentity,
    pub(super) intent_id: i64,
    pub(super) reservation_id: i64,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) storage_plan: crate::journal::StorageProvisioningPlan,
    pub(super) nonce: String,
}

pub(super) async fn provision_physical_intent(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    provision_physical_intent_with_sample(scope, nonce, nonce).await
}

pub(super) async fn provision_physical_intent_with_sample(
    scope: &ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    use crate::journal::{
        AssignmentKey, ReservationRequest, ReservationResult, StorageFilesystemKind,
        StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;
    use crate::launch::bootstrap::{
        BootstrapContainerRole, test_bootstrap_start_receipt, test_created_bootstrap_container,
        test_created_storage_object, test_observed_container_cgroup,
        test_observed_storage_attachment, test_observed_storage_role,
        test_verified_storage_resource_binding,
    };
    use crate::worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

    const JOB_MEMORY: u64 = 8_589_934_592;
    const BOOT_ID: &str = "11111111-2222-4333-8444-555555555555";
    let nonce = nonce.to_string().repeat(32);
    let sample_generation = sample_nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.clone(),
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }

    let names = WorkerIdentityNames::from_nonce(&nonce)?;
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let mut storage_plan = None;
    for (role, volume_role) in [
        (StorageRole::SharedRun, WorkerVolumeRole::Socket),
        (StorageRole::RunnerHome, WorkerVolumeRole::Home),
        (StorageRole::RunnerWork, WorkerVolumeRole::Work),
        (StorageRole::DindData, WorkerVolumeRole::DindData),
        (
            StorageRole::EngineMetadata,
            WorkerVolumeRole::RunnerMetadata,
        ),
    ] {
        let claim = scope
            .claim_storage_object_create(reservation_id, role)
            .await?
            .ok_or(HostError::Journal)?;
        let role_limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(role_limit.pool_id())?;
        let name = names.volume_name(volume_role);
        if claim.docker_name() != name
            || claim.profile_generation_id() != profile.generation_id()
            || claim.profile_digest() != profile.profile_digest()
            || claim.sample_generation_id() != sample_generation_id
            || claim.pool_id() != role_limit.pool_id()
            || claim.medium() != role_limit.medium()
            || claim.filesystem_kind()
                != if role == StorageRole::EngineMetadata {
                    StorageFilesystemKind::Btrfs
                } else {
                    StorageFilesystemKind::Tmpfs
                }
            || claim.inode_policy()
                != if role == StorageRole::EngineMetadata {
                    StorageInodePolicy::DynamicBtrfs
                } else {
                    StorageInodePolicy::Finite
                }
            || claim.charge_bytes() != role_limit.max_bytes()
            || claim.charge_inodes() != role_limit.max_inodes()
            || claim.retained_memory_bytes() != role_limit.retained_memory_bytes()
            || pool.inode_policy() != claim.inode_policy()
            || (role == StorageRole::EngineMetadata
                && claim.publication_policy_digest()
                    != profile
                        .controller_publication()
                        .map(|policy| policy.digest())
                        .as_deref())
            || (role != StorageRole::EngineMetadata && claim.publication_policy_digest().is_some())
        {
            return Err(HostError::Journal);
        }
        let proof = test_created_storage_object(
            reservation_id,
            intent_id,
            TEST_ENGINE_ID.to_owned(),
            docker_root_digest.clone(),
            role,
            name.clone(),
            name,
        )?;
        scope.record_storage_object_created(&claim, &proof).await?;
    }

    let runner_container_id = "a".repeat(64);
    let dind_container_id = "b".repeat(64);
    for (role, container_role, id) in [
        (
            BootstrapContainerRole::Runner,
            WorkerContainerRole::Runner,
            runner_container_id.as_str(),
        ),
        (
            BootstrapContainerRole::Dind,
            WorkerContainerRole::Dind,
            dind_container_id.as_str(),
        ),
    ] {
        let claim = scope
            .claim_bootstrap_container_create(reservation_id, role)
            .await?
            .ok_or(HostError::Journal)?;
        storage_plan.get_or_insert_with(|| claim.storage_plan().clone());
        let proof = test_created_bootstrap_container(
            reservation_id,
            intent_id,
            TEST_ENGINE_ID.to_owned(),
            docker_root_digest.clone(),
            &nonce,
            role,
            names.container_name(container_role),
            id.to_owned(),
        )?;
        scope
            .record_bootstrap_container_created(&claim, &proof)
            .await?;
    }

    let start = scope
        .claim_bootstrap_start(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let start_proof = test_bootstrap_start_receipt(
        reservation_id,
        intent_id,
        TEST_ENGINE_ID.to_owned(),
        docker_root_digest.clone(),
        runner_container_id.clone(),
        dind_container_id.clone(),
        true,
        true,
    )?;
    scope.record_bootstrap_started(&start, &start_proof).await?;

    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let mut roles = Vec::with_capacity(StorageRole::PHYSICAL.len());
    let mut role_digests = std::collections::BTreeMap::new();
    for role in StorageRole::PHYSICAL {
        let limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(limit.pool_id())?;
        let digest = role_filesystem_digest(role);
        let (total, free, available) = match limit.medium() {
            StorageMedium::Disk => (64_u64 << 30, 60_u64 << 30, 60_u64 << 30),
            StorageMedium::Tmpfs => (limit.max_bytes(), limit.max_bytes(), limit.max_bytes()),
        };
        let inode_counts = match pool.inode_policy() {
            StorageInodePolicy::DynamicBtrfs => (Some(0), Some(0), Some(0)),
            StorageInodePolicy::Finite => (
                Some(limit.max_inodes()),
                Some(limit.max_inodes()),
                Some(limit.max_inodes()),
            ),
            StorageInodePolicy::Unknown => return Err(HostError::Journal),
        };
        roles.push(test_observed_storage_role(
            role,
            limit.pool_id().to_owned(),
            digest.clone(),
            limit.medium(),
            if limit.medium() == StorageMedium::Disk {
                StorageFilesystemKind::Btrfs
            } else {
                StorageFilesystemKind::Tmpfs
            },
            (total, free, available),
            inode_counts,
            pool.inode_policy(),
            (
                limit.max_bytes(),
                limit.max_inodes(),
                limit.retained_memory_bytes(),
            ),
            (0, 0),
        )?);
        role_digests.insert(role, digest);
    }

    let mut attachments = Vec::new();
    for role in StorageRole::PHYSICAL {
        for (index, contract) in role.attachments().iter().enumerate() {
            let container_id = match contract.observer {
                StorageObserver::Runner => Some(runner_container_id.clone()),
                StorageObserver::Dind => Some(dind_container_id.clone()),
                StorageObserver::Probe => None,
            };
            let disk = profile.role_limit(role)?.medium() == StorageMedium::Disk;
            let (device, statx, magic, filesystem_id) = if disk {
                ((0, 38), (0, 41), 0x9123_683e, 11_u64)
            } else {
                let role_index = StorageRole::PHYSICAL
                    .iter()
                    .position(|item| *item == role)
                    .ok_or(HostError::Journal)?;
                let fs = i64::try_from(role_index)
                    .map_err(|_| HostError::Journal)?
                    .checked_add(30)
                    .ok_or(HostError::Journal)?;
                (
                    (0, 45),
                    (0, 45),
                    0x0102_1994,
                    u64::try_from(fs).map_err(|_| HostError::Journal)?,
                )
            };
            attachments.push(test_observed_storage_attachment(
                role,
                contract.observer,
                container_id,
                contract.target.to_owned(),
                contract.source_target.map(str::to_owned),
                contract.read_only,
                100 + (role as u64 * 2) + index as u64,
                device,
                statx,
                magic,
                filesystem_id,
                role_digests.get(&role).ok_or(HostError::Journal)?.clone(),
            )?);
        }
    }

    let cgroups = vec![
        test_observed_container_cgroup(
            BootstrapContainerRole::Runner,
            runner_container_id.clone(),
            "/test/runner".to_owned(),
            (Some(6_u64 << 30), 1_u64 << 20),
            (Some(100_000), 100_000),
            Some(2_048),
        )?,
        test_observed_container_cgroup(
            BootstrapContainerRole::Dind,
            dind_container_id.clone(),
            "/test/dind".to_owned(),
            (Some(2_u64 << 30), 1_u64 << 20),
            (Some(100_000), 100_000),
            Some(2_048),
        )?,
    ];
    let instance_binding_digest = "f".repeat(64);
    let binding = test_verified_storage_resource_binding(
        reservation_id,
        intent_id,
        TEST_ENGINE_ID.to_owned(),
        docker_root_digest.clone(),
        sample_generation_id.clone(),
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        runner_container_id.clone(),
        dind_container_id.clone(),
        instance_binding_digest.clone(),
        roles,
        attachments,
        cgroups,
        BOOT_ID.to_owned(),
    )?;
    scope
        .bind_storage_resources(reservation_id, binding)
        .await?;

    Ok(ProvisionedRunner {
        assignment,
        intent_id,
        reservation_id,
        runner_container_id,
        dind_container_id,
        docker_root_digest,
        sample_generation_id,
        instance_binding_digest,
        storage_plan: storage_plan.ok_or(HostError::Journal)?,
        nonce,
    })
}

fn role_filesystem_digest(role: crate::journal::StorageRole) -> String {
    use crate::journal::StorageRole;
    match role {
        StorageRole::RunnerRoot | StorageRole::DindRoot | StorageRole::EngineMetadata => {
            "a".repeat(64)
        }
        StorageRole::RunnerHome => "2".repeat(64),
        StorageRole::RunnerWork => "3".repeat(64),
        StorageRole::RunnerTmp => "4".repeat(64),
        StorageRole::RunnerShm => "5".repeat(64),
        StorageRole::SharedRun => "6".repeat(64),
        StorageRole::DindData => "7".repeat(64),
        StorageRole::DindTmp => "8".repeat(64),
        StorageRole::DindShm => "9".repeat(64),
        StorageRole::CacheLive | StorageRole::CacheStage => "a".repeat(64),
    }
}
