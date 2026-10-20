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

struct ProvisioningFixture<'a> {
    scope: &'a ConnectionJournal,
    engine_id: String,
    assignment: AssignmentIdentity,
    nonce: String,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
    filesystem_instance_digest: String,
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
        filesystem_instance_digest: "a".repeat(64),
    }
    .provision_with_initialization()
    .await
}

pub(crate) async fn provision_with_initialization_and_filesystem_digest(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
    filesystem_instance_digest: String,
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
        filesystem_instance_digest,
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
        filesystem_instance_digest: "a".repeat(64),
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
        filesystem_instance_digest: "a".repeat(64),
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
        filesystem_instance_digest: "a".repeat(64),
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
