use super::{fixture_resource_id, InitializerFixtureArtifacts, ProvisioningFixture};
use crate::error::HostError;
use crate::worker::storage_initialization::ObservedStorageInitialization;

impl ProvisioningFixture<'_> {
    pub(super) async fn record_storage_initialization(
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
        let helper_id = fixture_resource_id(self.reservation_id, "initializer");
        Box::pin(self.record_initializer_create(&claim, &helper_id)).await?;
        let start = Box::pin(self.record_initializer_start(&claim, &helper_id)).await?;
        let (handoff, observed) =
            Box::pin(self.record_initializer_result(&start, &helper_id)).await?;
        Ok(InitializerFixtureArtifacts {
            claim: start,
            handoff,
            observed,
            helper_id,
        })
    }

    pub(super) async fn record_initializer_create(
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

    pub(super) async fn record_initializer_start(
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

    pub(super) async fn record_initializer_result(
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

    pub(super) async fn record_initializer_remove(&self, helper_id: &str) -> Result<(), HostError> {
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
}
