use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::{
    AssignmentIdentity, StorageInitializationClaim, StorageProvisioningPlan,
    VerifiedMetadataInitializationHandoff,
};
use crate::worker::storage_initialization::{
    ObservedStorageInitialization, VerifiedStorageInitializationReady,
};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

use super::ProvisionedRunner;

#[path = "provisioning/binding.rs"]
mod binding;

const BOOT_ID: &str = "11111111-2222-4333-8444-555555555555";

struct ProvisioningFixture<'a> {
    scope: &'a ConnectionJournal,
    engine_id: String,
    assignment: AssignmentIdentity,
    nonce: String,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
}

pub(crate) struct ProvisionedStorageInitialization {
    pub(crate) provisioned: ProvisionedRunner,
    pub(crate) claim: StorageInitializationClaim,
    pub(crate) handoff: VerifiedMetadataInitializationHandoff,
    pub(crate) observed: ObservedStorageInitialization,
    pub(crate) ready: VerifiedStorageInitializationReady,
}

struct InitializerFixtureArtifacts {
    claim: StorageInitializationClaim,
    handoff: VerifiedMetadataInitializationHandoff,
    observed: ObservedStorageInitialization,
}

pub(super) async fn provision(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedRunner, HostError> {
    ProvisioningFixture {
        scope,
        engine_id: super::TEST_ENGINE_ID.to_owned(),
        assignment,
        nonce: nonce.to_string().repeat(32),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    }
    .provision()
    .await
}

pub(crate) async fn provision_with_initialization(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedStorageInitialization, HostError> {
    ProvisioningFixture {
        scope,
        engine_id: super::TEST_ENGINE_ID.to_owned(),
        assignment,
        nonce: nonce.to_string().repeat(32),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    }
    .provision_with_initialization()
    .await
}

pub(super) async fn provision_with_nonce(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: crate::journal::AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedRunner, HostError> {
    ProvisioningFixture {
        scope,
        engine_id: engine_id.to_owned(),
        assignment,
        nonce: nonce.to_owned(),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    }
    .provision()
    .await
}

pub(crate) async fn provision_with_nonce_and_initialization(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedStorageInitialization, HostError> {
    ProvisioningFixture {
        scope,
        engine_id: engine_id.to_owned(),
        assignment,
        nonce: nonce.to_owned(),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    }
    .provision_with_initialization()
    .await
}

pub(crate) async fn prepare_bootstrap_start_claim(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<
    (
        crate::journal::BootstrapStartClaim,
        crate::journal::VerifiedMetadataInitializationHandoff,
    ),
    HostError,
> {
    let fixture = ProvisioningFixture {
        scope,
        engine_id: engine_id.to_owned(),
        assignment,
        nonce: nonce.to_owned(),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    };
    let names = WorkerIdentityNames::from_nonce(nonce)?;
    let profile = fixture.profile().await?;
    fixture.record_storage_objects(&profile, &names).await?;
    let initialization = fixture.record_storage_initialization().await?;
    fixture.record_bootstrap_containers(&names).await?;
    let claim = scope
        .claim_bootstrap_start(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Ok((claim, initialization.handoff))
}

pub(crate) async fn prepare_initializer(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<crate::journal::StorageInitializationClaim, HostError> {
    use crate::worker::storage_initialization::StorageInitializationImage;

    prepare_initializer_state(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image = StorageInitializationImage::from_verified_release(&images)?;
    scope
        .claim_storage_initialization(
            reservation_id,
            &image,
            crate::journal::request_lifecycle::tests::helpers::test_initializer_limits()?,
        )
        .await?
        .ok_or(HostError::Journal)
}

pub(crate) async fn prepare_initializer_state(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<(), HostError> {
    let fixture = ProvisioningFixture {
        scope,
        engine_id: super::TEST_ENGINE_ID.to_owned(),
        assignment,
        nonce: nonce.to_string().repeat(32),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    };
    let names = WorkerIdentityNames::from_nonce(&fixture.nonce)?;
    let profile = fixture.profile().await?;
    fixture.record_storage_objects(&profile, &names).await?;
    Ok(())
}

impl ProvisioningFixture<'_> {
    async fn record_storage_network(&self) -> Result<(), HostError> {
        use crate::launch::bootstrap::VerifiedStorageNetworkCreated;
        use bollard::models::NetworkInspect;

        let claim = self
            .scope
            .claim_storage_network_create(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let inspected = NetworkInspect {
            name: Some(claim.network_name().to_owned()),
            id: Some("d".repeat(64)),
            scope: Some("local".to_owned()),
            driver: Some("bridge".to_owned()),
            enable_ipv4: Some(true),
            enable_ipv6: Some(false),
            internal: Some(false),
            attachable: Some(false),
            ingress: Some(false),
            labels: Some(
                claim
                    .labels()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
            ..Default::default()
        };
        let proof = VerifiedStorageNetworkCreated::test_created(&claim, &inspected)?;
        self.scope
            .record_storage_network_created(&claim, &proof)
            .await?;
        Ok(())
    }

    async fn provision(&self) -> Result<ProvisionedRunner, HostError> {
        Ok(self.provision_with_initialization().await?.provisioned)
    }

    async fn provision_with_initialization(
        &self,
    ) -> Result<ProvisionedStorageInitialization, HostError> {
        let names = WorkerIdentityNames::from_nonce(&self.nonce)?;
        let profile = self.profile().await?;
        self.record_storage_objects(&profile, &names).await?;
        let initialization = Box::pin(self.record_storage_initialization()).await?;
        self.record_storage_network().await?;
        let (runner_id, dind_id, storage_plan) = self.record_bootstrap_containers(&names).await?;
        let started = self.record_bootstrap_started(&runner_id, &dind_id).await?;
        let mount_handoff = crate::launch::bootstrap::test_bootstrap_mount_handoff(
            &initialization.handoff,
            &started,
            &"a".repeat(64),
        )?;
        Box::pin(
            self.scope
                .record_storage_initializer_pair_mounted(&initialization.handoff, &mount_handoff),
        )
        .await?;
        self.record_initializer_remove(&"c".repeat(64)).await?;
        let final_readiness = self
            .scope
            .metadata_initialization_ready(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let ready = VerifiedStorageInitializationReady::test_after_final_readiness(
            &initialization.claim,
            &initialization.handoff,
            initialization.observed.clone(),
            final_readiness,
        )?;
        let profile = self.profile().await?;
        let instance_binding_digest = self
            .record_storage_binding(&profile, &runner_id, &dind_id)
            .await?;
        Ok(ProvisionedStorageInitialization {
            provisioned: ProvisionedRunner {
                assignment: self.assignment.clone(),
                intent_id: self.intent_id,
                reservation_id: self.reservation_id,
                runner_container_id: runner_id,
                dind_container_id: dind_id,
                docker_root_digest: self.docker_root_digest.clone(),
                sample_generation_id: self.sample_generation_id.clone(),
                instance_binding_digest,
                storage_plan,
                nonce: self.nonce.clone(),
            },
            claim: initialization.claim,
            handoff: initialization.handoff,
            observed: initialization.observed,
            ready,
        })
    }

    async fn record_storage_initialization(
        &self,
    ) -> Result<InitializerFixtureArtifacts, HostError> {
        use crate::launch::bootstrap::test_created_storage_object;
        use crate::worker::storage_initialization::StorageInitializationImage;
        use crate::{
            journal::StorageRole,
            worker::{WorkerIdentityNames, WorkerVolumeRole},
        };

        let plan = self
            .scope
            .storage_provisioning_plan(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let images = crate::launch::verified_images::test_worker_images();
        let image = StorageInitializationImage::from_verified_release(&images)?;
        let names = WorkerIdentityNames::from_nonce(&self.nonce)?;
        let metadata_name = names.volume_name(WorkerVolumeRole::RunnerMetadata);
        let metadata = test_created_storage_object(
            self.reservation_id,
            plan.operation_id(),
            plan.engine_id().to_owned(),
            plan.docker_root_digest().to_owned(),
            StorageRole::EngineMetadata,
            metadata_name.clone(),
            metadata_name,
        )?;
        let claim = self
            .scope
            .claim_storage_initialization_v2(
                self.reservation_id,
                &image,
                crate::journal::request_lifecycle::tests::helpers::test_initializer_limits()?,
                &metadata,
            )
            .await?
            .ok_or(HostError::Journal)?;
        if self
            .scope
            .claim_storage_initialization_v2(
                self.reservation_id,
                &image,
                crate::journal::request_lifecycle::tests::helpers::test_initializer_limits()?,
                &metadata,
            )
            .await?
            .is_some()
        {
            return Err(HostError::Journal);
        }
        let helper_id = "c".repeat(64);
        Box::pin(self.record_initializer_create(&claim, &helper_id)).await?;
        let start = Box::pin(self.record_initializer_start(&claim, &helper_id)).await?;
        let (handoff, observed) =
            Box::pin(self.record_initializer_result(&start, &helper_id)).await?;
        Ok(InitializerFixtureArtifacts {
            claim: start,
            handoff,
            observed,
        })
    }

    async fn record_initializer_create(
        &self,
        claim: &crate::journal::StorageInitializationClaim,
        helper_id: &str,
    ) -> Result<(), HostError> {
        use crate::worker::storage_initialization::{
            StorageInitializerCreateResponse, VerifiedStorageInitializerCreated,
        };
        let foreign = VerifiedStorageInitializerCreated::test_created_with_engine(
            claim,
            helper_id,
            "22222222-2222-4222-8222-222222222222",
        );
        if self
            .scope
            .record_storage_initializer_created(claim, &foreign)
            .await
            .is_ok()
        {
            return Err(HostError::Journal);
        }
        self.scope
            .record_storage_initializer_create_dispatched(claim)
            .await?;
        self.scope
            .record_storage_initializer_create_response_received(claim)
            .await?;
        let response = StorageInitializerCreateResponse::test_response(claim, helper_id);
        Box::pin(
            self.scope
                .record_storage_initializer_create_response(claim, &response),
        )
        .await?;
        let created = VerifiedStorageInitializerCreated::test_created(claim, helper_id);
        Box::pin(
            self.scope
                .record_storage_initializer_created(claim, &created),
        )
        .await
    }

    async fn record_initializer_start(
        &self,
        claim: &crate::journal::StorageInitializationClaim,
        helper_id: &str,
    ) -> Result<crate::journal::StorageInitializationClaim, HostError> {
        use crate::worker::storage_initialization::VerifiedStorageInitializerStarted;
        let start = self
            .scope
            .claim_storage_initializer_start(claim.reservation_id())
            .await?
            .ok_or(HostError::Journal)?;
        if self
            .scope
            .claim_storage_initializer_start(claim.reservation_id())
            .await?
            .is_some()
        {
            return Err(HostError::Journal);
        }
        let started = VerifiedStorageInitializerStarted::test_started(&start, helper_id);
        self.scope
            .record_storage_initializer_started(&start, &started)
            .await?;
        if self
            .scope
            .record_storage_initializer_started(&start, &started)
            .await
            .is_ok()
        {
            return Err(HostError::Journal);
        }
        Ok(start)
    }

    async fn record_initializer_result(
        &self,
        start: &crate::journal::StorageInitializationClaim,
        helper_id: &str,
    ) -> Result<
        (
            crate::journal::VerifiedMetadataInitializationHandoff,
            ObservedStorageInitialization,
        ),
        HostError,
    > {
        use crate::worker::storage_initialization::{
            VerifiedStorageInitializerResult, VerifiedStorageInitializerStarted,
            request_result::parse_ready,
        };
        let record = format!(
            r#"{{"schema":2,"operation_id":"{}","home":{{"target":"/velnor/init/home","mount_id":31,"parent_mount_id":1,"device_major":0,"device_minor":48,"inode":61,"uid":1000,"gid":1000,"mode":493}},"work":{{"target":"/velnor/init/work","mount_id":32,"parent_mount_id":1,"device_major":0,"device_minor":48,"inode":62,"uid":1000,"gid":1000,"mode":493}},"metadata":{{"target":"/velnor/init/metadata","mount_id":33,"parent_mount_id":1,"device_major":0,"device_minor":48,"inode":63,"uid":0,"gid":0,"mode":365}},"runtime_identity":{{"target":"/velnor/init/metadata/runtime-identity","mount_id":33,"parent_mount_id":33,"device_major":0,"device_minor":48,"inode":64,"uid":0,"gid":0,"mode":365}},"job_facts":{{"target":"/velnor/init/metadata/job-facts","mount_id":33,"parent_mount_id":33,"device_major":0,"device_minor":48,"inode":65,"uid":0,"gid":0,"mode":365}},"source_proof":{{"target":"/velnor/init/metadata/source-proof","mount_id":33,"parent_mount_id":33,"device_major":0,"device_minor":48,"inode":66,"uid":0,"gid":0,"mode":493}},"control":{{"target":"/velnor/init/metadata/source-proof/control","mount_id":33,"parent_mount_id":33,"device_major":0,"device_minor":48,"inode":67,"uid":0,"gid":0,"mode":365}},"inbox":{{"target":"/velnor/init/home/source-proof/inbox","mount_id":31,"parent_mount_id":31,"device_major":0,"device_minor":48,"inode":68,"uid":1000,"gid":1000,"mode":448}}}}"#,
            start.operation_key()
        );
        let output = format!("{record}\n");
        let observed = parse_ready(start.operation_key(), output.as_bytes())?;
        let result =
            VerifiedStorageInitializerResult::test_after_validated_result(start, &observed)?;
        self.scope
            .record_storage_initializer_result_v2(start, &observed, &result)
            .await?;
        let started = VerifiedStorageInitializerStarted::test_started(start, helper_id);
        let awaiting = crate::worker::storage_initialization::VerifiedStorageInitializerAwaitingHandoff::test_after_ready(
            self.scope.connection_id().as_str(), start, &started, &result,
        )?;
        for role in [
            crate::launch::bootstrap::BootstrapContainerRole::Runner,
            crate::launch::bootstrap::BootstrapContainerRole::Dind,
        ] {
            if self
                .scope
                .claim_bootstrap_container_create(self.reservation_id, role)
                .await?
                .is_some()
            {
                return Err(HostError::Journal);
            }
        }
        let handoff = self
            .scope
            .record_storage_initializer_ready_held(start, &awaiting)
            .await?;
        Ok((handoff, observed))
    }

    async fn record_initializer_remove(&self, helper_id: &str) -> Result<(), HostError> {
        use crate::worker::storage_initialization::VerifiedStorageInitializerRemoved;
        let remove = self
            .scope
            .claim_storage_initializer_removal(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        if self
            .scope
            .claim_storage_initializer_removal(self.reservation_id)
            .await?
            .is_some()
        {
            return Err(HostError::Journal);
        }
        let removed = VerifiedStorageInitializerRemoved::test_removed(&remove, helper_id);
        self.scope
            .record_storage_initializer_removed(&remove, &removed)
            .await
    }

    async fn profile(&self) -> Result<crate::journal::StorageProfile, HostError> {
        self.scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)
    }

    async fn record_storage_objects(
        &self,
        profile: &crate::journal::StorageProfile,
        names: &WorkerIdentityNames,
    ) -> Result<(), HostError> {
        use crate::journal::StorageRole;
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
            self.record_storage_object(profile, names, role, volume_role)
                .await?;
        }
        Ok(())
    }

    async fn record_storage_object(
        &self,
        profile: &crate::journal::StorageProfile,
        names: &WorkerIdentityNames,
        role: crate::journal::StorageRole,
        volume_role: WorkerVolumeRole,
    ) -> Result<(), HostError> {
        use crate::journal::{StorageFilesystemKind, StorageInodePolicy, StorageRole};
        use crate::launch::bootstrap::test_created_storage_object;

        let claim = self
            .scope
            .claim_storage_object_create(self.reservation_id, role)
            .await?
            .ok_or(HostError::Journal)?;
        let role_limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(role_limit.pool_id())?;
        let name = names.volume_name(volume_role);
        let expected_kind = if role == StorageRole::EngineMetadata {
            StorageFilesystemKind::Btrfs
        } else {
            StorageFilesystemKind::Tmpfs
        };
        let expected_inode_policy = if role == StorageRole::EngineMetadata {
            StorageInodePolicy::DynamicBtrfs
        } else {
            StorageInodePolicy::Finite
        };
        let publication_matches = if role == StorageRole::EngineMetadata {
            match profile.controller_publication() {
                Some(policy) => {
                    let digest = policy.digest();
                    claim.publication_policy_digest() == Some(digest.as_str())
                }
                None => false,
            }
        } else {
            claim.publication_policy_digest().is_none()
        };
        if claim.docker_name() != name
            || claim.profile_generation_id() != profile.generation_id()
            || claim.profile_digest() != profile.profile_digest()
            || claim.sample_generation_id() != self.sample_generation_id
            || claim.pool_id() != role_limit.pool_id()
            || claim.medium() != role_limit.medium()
            || claim.filesystem_kind() != expected_kind
            || claim.inode_policy() != expected_inode_policy
            || claim.charge_bytes() != role_limit.max_bytes()
            || claim.charge_inodes() != role_limit.max_inodes()
            || claim.retained_memory_bytes() != role_limit.retained_memory_bytes()
            || pool.inode_policy() != claim.inode_policy()
            || !publication_matches
        {
            return Err(HostError::Journal);
        }
        let proof = test_created_storage_object(
            self.reservation_id,
            self.intent_id,
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
            role,
            name.clone(),
            name,
        )?;
        self.scope
            .record_storage_object_created(&claim, &proof)
            .await
    }

    async fn record_bootstrap_containers(
        &self,
        names: &WorkerIdentityNames,
    ) -> Result<(String, String, StorageProvisioningPlan), HostError> {
        use crate::launch::bootstrap::{BootstrapContainerRole, test_created_bootstrap_container};
        use crate::worker::WorkerContainerRole;

        let runner_id = "a".repeat(64);
        let dind_id = "b".repeat(64);
        let mut storage_plan = None;
        for (role, container_role, id) in [
            (
                BootstrapContainerRole::Runner,
                WorkerContainerRole::Runner,
                &runner_id,
            ),
            (
                BootstrapContainerRole::Dind,
                WorkerContainerRole::Dind,
                &dind_id,
            ),
        ] {
            let claim = self
                .scope
                .claim_bootstrap_container_create(self.reservation_id, role)
                .await?
                .ok_or(HostError::Journal)?;
            storage_plan.get_or_insert_with(|| claim.storage_plan().clone());
            let proof = test_created_bootstrap_container(
                self.reservation_id,
                self.intent_id,
                self.engine_id.clone(),
                self.docker_root_digest.clone(),
                &self.nonce,
                role,
                names.container_name(container_role),
                id.clone(),
            )?;
            self.scope
                .record_bootstrap_container_created(&claim, &proof)
                .await?;
        }
        Ok((runner_id, dind_id, storage_plan.ok_or(HostError::Journal)?))
    }

    async fn record_bootstrap_started(
        &self,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<crate::launch::bootstrap::BootstrapStartReceipt, HostError> {
        use crate::launch::bootstrap::test_bootstrap_start_receipt;

        let start = self
            .scope
            .claim_bootstrap_start(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let proof = test_bootstrap_start_receipt(
            self.reservation_id,
            self.intent_id,
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
            runner_id.to_owned(),
            dind_id.to_owned(),
            true,
            true,
        )?;
        self.scope.record_bootstrap_started(&start, &proof).await?;
        Ok(proof)
    }
}
