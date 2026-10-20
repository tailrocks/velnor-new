use super::{
    ProvisionedRunner, ProvisionedStorageInitialization, ProvisioningFixture, fixture_resource_id,
};
use crate::error::HostError;
use crate::worker::WorkerIdentityNames;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;

impl ProvisioningFixture<'_> {
    pub(super) async fn record_storage_network(&self) -> Result<(), HostError> {
        use crate::launch::bootstrap::VerifiedStorageNetworkCreated;
        use bollard::models::NetworkInspect;

        let claim = self
            .scope
            .claim_storage_network_create(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let inspected = NetworkInspect {
            name: Some(claim.network_name().to_owned()),
            id: Some(fixture_resource_id(self.reservation_id, "network")),
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

    pub(super) async fn provision(&self) -> Result<ProvisionedRunner, HostError> {
        Ok(self.provision_with_initialization().await?.provisioned)
    }

    pub(super) async fn provision_with_initialization(
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
            &self.filesystem_instance_digest,
        )?;
        Box::pin(
            self.scope
                .record_storage_initializer_pair_mounted(&initialization.handoff, &mount_handoff),
        )
        .await?;
        self.record_initializer_remove(&initialization.helper_id)
            .await?;
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
}
