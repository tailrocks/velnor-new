use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;

#[path = "inventory/containers.rs"]
mod containers;
#[path = "inventory/create_spec.rs"]
mod create_spec;

/// This snapshot stores resource names and IDs for pre-JIT cleanup.
pub(crate) struct PreJitCleanupInventory {
    bootstrap_containers: Vec<PreJitBootstrapContainer>,
    storage_objects: Vec<PreJitStorageObject>,
    network: PreJitNetworkIdentity,
    initializer: PreJitInitializerIdentity,
    storage_plan: super::super::StorageProvisioningPlan,
    create_specs: Vec<PreJitCreateSpec>,
}

impl PreJitCleanupInventory {
    pub(crate) fn bootstrap_containers(&self) -> &[PreJitBootstrapContainer] {
        &self.bootstrap_containers
    }

    pub(crate) fn storage_objects(&self) -> &[PreJitStorageObject] {
        &self.storage_objects
    }

    pub(crate) fn network(&self) -> &PreJitNetworkIdentity {
        &self.network
    }

    pub(crate) fn initializer(&self) -> &PreJitInitializerIdentity {
        &self.initializer
    }

    pub(crate) fn storage_plan(&self) -> &super::super::StorageProvisioningPlan {
        &self.storage_plan
    }

    pub(crate) fn create_specs(&self) -> &[PreJitCreateSpec] {
        &self.create_specs
    }
}

pub(crate) struct PreJitCreateSpec {
    role: String,
    name: String,
    image_runtime_id: String,
    image_source_sha: String,
    budget: crate::worker::ResourceBudgetConfig,
    spec_sha256: String,
}

impl PreJitCreateSpec {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn image_runtime_id(&self) -> &str {
        &self.image_runtime_id
    }
    pub(crate) fn image_source_sha(&self) -> &str {
        &self.image_source_sha
    }
    pub(crate) const fn budget(&self) -> crate::worker::ResourceBudgetConfig {
        self.budget
    }
    pub(crate) fn spec_sha256(&self) -> &str {
        &self.spec_sha256
    }
}

pub(crate) struct PreJitBootstrapContainer {
    role: String,
    name: String,
    id: Option<String>,
    create_state: String,
}

impl PreJitBootstrapContainer {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub(crate) fn create_state(&self) -> &str {
        &self.create_state
    }
}

pub(crate) struct PreJitStorageObject {
    role: String,
    name: String,
    id: Option<String>,
    create_state: String,
}

impl PreJitStorageObject {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub(crate) fn create_state(&self) -> &str {
        &self.create_state
    }
}

pub(crate) struct PreJitNetworkIdentity {
    name: String,
    id: Option<String>,
    labels_json: String,
    create_state: String,
    remove_state: String,
}

impl PreJitNetworkIdentity {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub(crate) fn labels_json(&self) -> &str {
        &self.labels_json
    }
    pub(crate) fn create_state(&self) -> &str {
        &self.create_state
    }
    pub(crate) fn remove_state(&self) -> &str {
        &self.remove_state
    }
}

pub(crate) struct PreJitInitializerIdentity {
    operation_id: i64,
    generation: i64,
    operation_key: String,
    helper_name: String,
    helper_id: Option<String>,
    engine_id: String,
    docker_root_digest: String,
    image_runtime_id: String,
    image_source_sha: String,
    home_volume: String,
    work_volume: String,
    metadata_volume: String,
    metadata_volume_id: Option<String>,
    cpu_nano: i64,
    memory_bytes: i64,
    pids_limit: i64,
    layout_version: i64,
}

impl PreJitInitializerIdentity {
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    pub(crate) fn helper_name(&self) -> &str {
        &self.helper_name
    }
    pub(crate) fn helper_id(&self) -> Option<&str> {
        self.helper_id.as_deref()
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn image_runtime_id(&self) -> &str {
        &self.image_runtime_id
    }
    pub(crate) fn image_source_sha(&self) -> &str {
        &self.image_source_sha
    }
    pub(crate) fn home_volume(&self) -> &str {
        &self.home_volume
    }
    pub(crate) fn work_volume(&self) -> &str {
        &self.work_volume
    }
    pub(crate) fn metadata_volume(&self) -> &str {
        &self.metadata_volume
    }
    pub(crate) fn metadata_volume_id(&self) -> Option<&str> {
        self.metadata_volume_id.as_deref()
    }
    pub(crate) const fn cpu_nano(&self) -> i64 {
        self.cpu_nano
    }
    pub(crate) const fn memory_bytes(&self) -> i64 {
        self.memory_bytes
    }
    pub(crate) const fn pids_limit(&self) -> i64 {
        self.pids_limit
    }
    pub(crate) const fn layout_version(&self) -> i64 {
        self.layout_version
    }
}

impl super::super::ConnectionJournal {
    /// Read identities only while the cleanup lease still owns the saved snapshot.
    pub(crate) async fn pre_jit_cleanup_inventory(
        &self,
        claim: &super::PreJitCleanupClaim,
    ) -> Result<Option<PreJitCleanupInventory>, HostError> {
        if !self.validate_pre_jit_cleanup_claim(claim).await? {
            return Ok(None);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) =
                super::hold_context(connection, &connection_id, claim.reservation_id).await?
            else {
                return Ok(None);
            };
            if super::snapshot::snapshot_digest(connection, claim.reservation_id, &context).await?
                != claim.snapshot_sha256
            {
                return Ok(None);
            }
            let bootstrap_containers =
                containers::read_bootstrap(connection, claim.reservation_id).await?;
            let storage_objects =
                containers::read_objects(connection, claim.reservation_id).await?;
            let Some(network) = read_network(connection, claim.reservation_id).await? else {
                return Ok(None);
            };
            let Some(initializer) = read_initializer(connection, claim.reservation_id).await?
            else {
                return Ok(None);
            };
            let storage_plan =
                super::super::storage_plan::load(connection, claim.reservation_id, &context)
                    .await?;
            let create_specs =
                create_spec::read_create_specs(connection, claim.reservation_id).await?;
            if bootstrap_containers.len() != 2 || storage_objects.len() != 5 {
                return Ok(None);
            }
            if !create_spec::valid_create_specs(&bootstrap_containers, &create_specs) {
                return Ok(None);
            }
            Ok(Some(PreJitCleanupInventory {
                bootstrap_containers,
                storage_objects,
                network,
                initializer,
                storage_plan,
                create_specs,
            }))
        })
        .await
    }
}

async fn read_network(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<PreJitNetworkIdentity>, HostError> {
    let mut rows = connection.query(
        "SELECT network_name, network_id, labels_json, create_state, remove_state FROM capacity_reservation_networks WHERE reservation_id = ?1",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let network = PreJitNetworkIdentity {
        name: row.get(0).map_err(|_| HostError::Journal)?,
        id: row.get(1).map_err(|_| HostError::Journal)?,
        labels_json: row.get(2).map_err(|_| HostError::Journal)?,
        create_state: row.get(3).map_err(|_| HostError::Journal)?,
        remove_state: row.get(4).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(network))
}

async fn read_initializer(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<PreJitInitializerIdentity>, HostError> {
    let mut rows = connection.query(
        "SELECT operation_id, generation, operation_key, helper_name, helper_id, engine_id, docker_root_digest, image_runtime_id, image_source_sha, home_volume, work_volume, metadata_volume, metadata_volume_id, create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, nano_cpus, memory_bytes, pids_limit, layout_version FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let initializer = PreJitInitializerIdentity {
        operation_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        operation_key: row.get(2).map_err(|_| HostError::Journal)?,
        helper_name: row.get(3).map_err(|_| HostError::Journal)?,
        helper_id: row.get(4).map_err(|_| HostError::Journal)?,
        engine_id: row.get(5).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(6).map_err(|_| HostError::Journal)?,
        image_runtime_id: row.get(7).map_err(|_| HostError::Journal)?,
        image_source_sha: row.get(8).map_err(|_| HostError::Journal)?,
        home_volume: row.get(9).map_err(|_| HostError::Journal)?,
        work_volume: row.get(10).map_err(|_| HostError::Journal)?,
        metadata_volume: row.get(11).map_err(|_| HostError::Journal)?,
        metadata_volume_id: row.get(12).map_err(|_| HostError::Journal)?,
        cpu_nano: row.get(19).map_err(|_| HostError::Journal)?,
        memory_bytes: row.get(20).map_err(|_| HostError::Journal)?,
        pids_limit: row.get(21).map_err(|_| HostError::Journal)?,
        layout_version: row.get(22).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(initializer))
}
