use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::{
    AssignmentIdentity, StorageInitializationClaim, VerifiedMetadataInitializationHandoff,
};
use crate::worker::WorkerIdentityNames;
use crate::worker::storage_initialization::{
    ObservedStorageInitialization, VerifiedStorageInitializationReady,
};
use sha2::{Digest, Sha256};

use super::ProvisionedRunner;

#[path = "provisioning/binding.rs"]
mod binding;

const BOOT_ID: &str = "11111111-2222-4333-8444-555555555555";

pub(crate) struct ProvisioningFixture<'a> {
    pub(crate) scope: &'a ConnectionJournal,
    pub(crate) engine_id: String,
    pub(crate) assignment: AssignmentIdentity,
    pub(crate) nonce: String,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) docker_root_digest: String,
    pub(crate) sample_generation_id: String,
    pub(crate) filesystem_instance_digest: String,
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
    helper_id: String,
}

fn fixture_resource_id(reservation_id: i64, role: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-test-resource-id-v1\0");
    digest.update(reservation_id.to_be_bytes());
    digest.update((role.len() as u64).to_be_bytes());
    digest.update(role.as_bytes());
    let mut encoded = String::with_capacity(64);
    for byte in digest.finalize() {
        encoded.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        encoded.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    encoded
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
        filesystem_instance_digest: "a".repeat(64),
    }
    .provision()
    .await
}

pub(crate) async fn provision_with_initialization(
    fixture: ProvisioningFixture<'_>,
) -> Result<ProvisionedStorageInitialization, HostError> {
    fixture.provision_with_initialization().await
}

pub(crate) async fn provision_with_engine_and_filesystem_digest(
    fixture: ProvisioningFixture<'_>,
) -> Result<ProvisionedStorageInitialization, HostError> {
    fixture.provision_with_initialization().await
}

pub(super) async fn provision_with_nonce(
    fixture: ProvisioningFixture<'_>,
) -> Result<ProvisionedRunner, HostError> {
    fixture.provision().await
}

pub(crate) async fn prepare_bootstrap_start_claim(
    fixture: ProvisioningFixture<'_>,
) -> Result<
    (
        crate::journal::BootstrapStartClaim,
        crate::journal::VerifiedMetadataInitializationHandoff,
    ),
    HostError,
> {
    let names = WorkerIdentityNames::from_nonce(&fixture.nonce)?;
    let profile = fixture.profile().await?;
    fixture.record_storage_objects(&profile, &names).await?;
    let initialization = fixture.record_storage_initialization().await?;
    fixture.record_bootstrap_containers(&names).await?;
    let claim = fixture
        .scope
        .claim_bootstrap_start(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Ok((claim, initialization.handoff))
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
        filesystem_instance_digest: "a".repeat(64),
    };
    let names = WorkerIdentityNames::from_nonce(&fixture.nonce)?;
    let profile = fixture.profile().await?;
    fixture.record_storage_objects(&profile, &names).await?;
    Ok(())
}

#[path = "provisioning/bootstrap.rs"]
mod bootstrap;
#[path = "provisioning/initializer.rs"]
mod initializer;
#[path = "provisioning/setup.rs"]
mod setup;
#[path = "provisioning/storage_objects.rs"]
mod storage_objects;
