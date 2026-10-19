use super::{fixture_resource_id, ProvisioningFixture};
use crate::error::HostError;
use crate::journal::StorageProvisioningPlan;
use crate::worker::WorkerIdentityNames;

impl ProvisioningFixture<'_> {
    pub(super) async fn record_bootstrap_containers(
        &self,
        names: &WorkerIdentityNames,
    ) -> Result<(String, String, StorageProvisioningPlan), HostError> {
        use crate::launch::bootstrap::{BootstrapContainerRole, test_created_bootstrap_container};
        use crate::worker::WorkerContainerRole;

        let runner_id = fixture_resource_id(self.reservation_id, "runner");
        let dind_id = fixture_resource_id(self.reservation_id, "dind");
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

    pub(super) async fn record_bootstrap_started(
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
