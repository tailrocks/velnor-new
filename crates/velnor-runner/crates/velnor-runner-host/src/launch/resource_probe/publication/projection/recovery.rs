//! Persist the exact nonsecret Docker projection needed after restart.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velnor_resource_probe::{
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::JobFactsRuntimeIdentityDirectory,
};

use crate::error::HostError;
use crate::journal::{ExecutionPublicationParent, PublicationKind};

use super::super::request::PublicationRequest;
use super::PublicationProjection;

const RECOVERY_SCHEMA: u8 = 2;
pub(super) const MAX_RECOVERY_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::launch::resource_probe::publication) struct PublicationRecoveryProjection {
    schema_version: u8,
    operation_id: String,
    instance_id: String,
    engine_id: String,
    docker_root_digest: String,
    operation_name: String,
    platform: String,
    source_revision: String,
    runtime_image_id: String,
    image_binding_digest: String,
    projection_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    metadata_volume_mountpoint: String,
    expected_payload_sha256: String,
    expected_guest_image_sha256: Option<String>,
    expected_runtime_identity: Option<JobFactsRuntimeIdentityDirectory>,
    parent_kind: String,
    parent_operation_id: i64,
    reservation_id: i64,
    connection_id: String,
    runner_id: i64,
    runner_container_id: String,
    dind_container_id: String,
    metadata_layout_version: i64,
    metadata_layout_digest: String,
    guest_boot_id: String,
    publisher_mode: String,
    runner_directory: ExecutionChannelDirectory,
    config: bollard::models::ContainerCreateBody,
    expected_config: bollard::models::ContainerConfig,
}

impl PublicationRecoveryProjection {
    pub(in crate::launch::resource_probe::publication) fn from_live(
        projection: &PublicationProjection,
        metadata_volume_id: &str,
        metadata_volume_mountpoint: &str,
        parent: &ExecutionPublicationParent,
    ) -> Result<Self, HostError> {
        let request = projection.request.as_ref().ok_or(HostError::Identity)?;
        if parent.engine_id() != projection.engine_id
            || parent.docker_root_digest() != projection.root.digest()
            || parent.metadata_volume_name() != projection.metadata_volume
            || parent.metadata_volume_id() != metadata_volume_id
            || parent.metadata_layout_version() != 2
            || !lower_hex_id(parent.metadata_layout_digest())
            || request.payload_sha256() != parent.payload_sha256()
        {
            return Err(HostError::Identity);
        }
        let image_id = projection
            .config
            .image
            .as_deref()
            .ok_or(HostError::Identity)?;
        let (expected_guest_image_sha256, expected_runtime_identity) = match request {
            PublicationRequest::Control(_) => (None, None),
            PublicationRequest::JobFacts(value) => (
                Some(value.guest_image_sha256.clone()),
                Some(value.expected_runner_runtime_identity.clone()),
            ),
        };
        let value = Self {
            schema_version: RECOVERY_SCHEMA,
            operation_id: projection.operation_id.clone(),
            instance_id: projection.instance_id.clone(),
            engine_id: projection.engine_id.clone(),
            docker_root_digest: projection.root.digest().to_owned(),
            operation_name: projection.name.clone(),
            platform: "linux/amd64".to_owned(),
            source_revision: projection.source_revision.clone(),
            runtime_image_id: image_id.to_owned(),
            image_binding_digest: projection.image_binding_digest.clone(),
            projection_digest: projection.digest.clone(),
            metadata_volume_name: projection.metadata_volume.clone(),
            metadata_volume_id: metadata_volume_id.to_owned(),
            metadata_volume_mountpoint: metadata_volume_mountpoint.to_owned(),
            expected_payload_sha256: request.payload_sha256().to_owned(),
            expected_guest_image_sha256,
            expected_runtime_identity,
            parent_kind: match parent.parent_kind() {
                PublicationKind::Challenge => "challenge",
                PublicationKind::Decision => "decision",
                PublicationKind::JobFacts => "job-facts",
            }
            .to_owned(),
            parent_operation_id: parent.operation_id(),
            reservation_id: parent.reservation_id(),
            connection_id: parent.connection_id().to_owned(),
            runner_id: parent.runner_id(),
            runner_container_id: parent.runner_container_id().to_owned(),
            dind_container_id: parent.dind_container_id().to_owned(),
            metadata_layout_version: parent.metadata_layout_version(),
            metadata_layout_digest: parent.metadata_layout_digest().to_owned(),
            guest_boot_id: parent.guest_boot_id().to_owned(),
            publisher_mode: projection.publisher_mode.clone(),
            runner_directory: request.directory().clone(),
            config: projection.config.clone(),
            expected_config: projection.expected_config.clone(),
        };
        value.validate()?;
        Ok(value)
    }

    pub(in crate::launch::resource_probe::publication) fn into_projection(
        self,
        root_path: &str,
    ) -> Result<PublicationProjection, HostError> {
        self.validate()?;
        let root = super::super::super::projection::DockerRoot::parse(root_path)?;
        let name = self.operation_name.clone();
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(name.clone()),
            platform: self.platform.clone(),
        };
        Ok(PublicationProjection {
            name,
            operation_id: self.operation_id,
            root,
            engine_id: self.engine_id,
            metadata_volume: self.metadata_volume_name,
            options,
            config: self.config,
            expected_config: self.expected_config,
            digest: self.projection_digest,
            request: None,
            publisher_mode: self.publisher_mode,
            instance_id: self.instance_id,
            source_revision: self.source_revision,
            image_binding_digest: self.image_binding_digest,
        })
    }

    pub(in crate::launch::resource_probe::publication) fn encode(
        &self,
    ) -> Result<String, HostError> {
        self.validate()?;
        let bytes = crate::launch::resource_probe::projection::canonical::value(self)
            .map_err(|_| HostError::Journal)?;
        if bytes.len() > MAX_RECOVERY_BYTES {
            return Err(HostError::Frame);
        }
        String::from_utf8(bytes).map_err(|_| HostError::Journal)
    }

    pub(in crate::launch::resource_probe::publication) fn decode(
        bytes: &str,
        expected_sha256: &str,
    ) -> Result<Self, HostError> {
        if bytes.is_empty() || bytes.len() > MAX_RECOVERY_BYTES {
            return Err(HostError::Identity);
        }
        let expected = lower_hex(&Sha256::digest(bytes.as_bytes()));
        if expected != expected_sha256 {
            return Err(HostError::Identity);
        }
        let value: Self = serde_json::from_str(bytes).map_err(|_| HostError::Identity)?;
        value.validate()?;
        if value.encode()? != bytes {
            return Err(HostError::Identity);
        }
        Ok(value)
    }

    fn validate(&self) -> Result<(), HostError> {
        let expected_name = format!("velnor-execution-publication-{}", self.operation_id);
        let expected_config = self.config.image.as_deref().ok_or(HostError::Identity)?;
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(self.operation_name.clone()),
            platform: self.platform.clone(),
        };
        let digest =
            crate::launch::resource_probe::projection::canonical::pair(&options, &self.config)
                .map(|bytes| lower_hex(&Sha256::digest(bytes)))
                .map_err(|_| HostError::Journal)?;
        self.validate_projection_identity(&expected_name, expected_config, &digest)?;
        self.validate_mount_and_payload()
    }

    fn validate_mount_and_payload(&self) -> Result<(), HostError> {
        if match (
            self.parent_kind.as_str(),
            &self.expected_guest_image_sha256,
            &self.expected_runtime_identity,
        ) {
            ("job-facts", Some(hash), Some(identity)) => {
                !lower_hex_id(hash)
                    || identity.inode == 0
                    || identity.owner_uid != 0
                    || identity.owner_gid != 0
                    || identity.mode != 0o555
            }
            ("job-facts", _, _) => true,
            (_, None, None) => false,
            _ => true,
        } || self.runner_directory.target
            != if self.parent_kind == "job-facts" {
                "/opt/velnor/job-facts"
            } else {
                "/opt/velnor/source-proof/control"
            }
            || self.runner_directory.inode == 0
            || self.runner_directory.mount_id == 0
            || self.runner_directory.statx_mount_id != self.runner_directory.mount_id
            || self.runner_directory.owner_uid != 0
            || self.runner_directory.owner_gid != 0
            || self.runner_directory.mode != 0o555
            || !self.runner_directory.read_only
            || self.runner_directory.filesystem_type_magic == 0
            || self.runner_directory.filesystem_id == 0
            || !valid_mount_root(&self.runner_directory.mount_root)
            || self.runner_directory.mount_source.is_empty()
            || self
                .runner_directory
                .mount_source
                .chars()
                .any(char::is_control)
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    fn validate_projection_identity(
        &self,
        expected_name: &str,
        expected_config: &str,
        digest: &str,
    ) -> Result<(), HostError> {
        if self.schema_version != RECOVERY_SCHEMA
            || !lower_hex_id(&self.operation_id)
            || !lower_hex_id(&self.instance_id)
            || self.operation_name != expected_name
            || self.platform != "linux/amd64"
            || self.source_revision.len() != 40
            || !self
                .source_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !self.runtime_image_id.starts_with("sha256:")
            || self.runtime_image_id != expected_config
            || !self
                .runtime_image_id
                .strip_prefix("sha256:")
                .is_some_and(|digest| digest.len() == 64 && lower_hex_string(digest))
            || !lower_hex_id(&self.image_binding_digest)
            || !lower_hex_id(&self.docker_root_digest)
            || !lower_hex_id(&self.projection_digest)
            || digest != self.projection_digest
            || self.metadata_volume_name.is_empty()
            || self.metadata_volume_name.len() > 64
            || self.metadata_volume_id != self.metadata_volume_name
            || !matches!(
                self.parent_kind.as_str(),
                "challenge" | "decision" | "job-facts"
            )
            || !matches!(
                self.publisher_mode.as_str(),
                "execution_publication" | "job_facts_publication"
            )
            || (self.parent_kind == "job-facts") != (self.publisher_mode == "job_facts_publication")
            || self.reservation_id <= 0
            || self.parent_operation_id <= 0
            || self.connection_id.is_empty()
            || self.runner_id <= 0
            || self.runner_container_id.len() < 12
            || self.runner_container_id.len() > 128
            || self.dind_container_id.len() < 12
            || self.dind_container_id.len() > 128
            || self.runner_container_id == self.dind_container_id
            || self.metadata_layout_version != 2
            || !lower_hex_id(&self.metadata_layout_digest)
            || !valid_boot_id(&self.guest_boot_id)
            || !valid_mountpoint(&self.metadata_volume_mountpoint)
            || !lower_hex_id(&self.expected_payload_sha256)
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

mod accessors;
mod validation;
use self::validation::{
    lower_hex, lower_hex_id, lower_hex_string, valid_boot_id, valid_mount_root, valid_mountpoint,
};

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;
