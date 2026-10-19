//! Persist the exact nonsecret Docker projection needed after restart.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory;

use crate::error::HostError;
use crate::journal::{ExecutionPublicationParent, PublicationKind};

use super::PublicationProjection;

const RECOVERY_SCHEMA: u8 = 1;
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
    expected_file_sha256: String,
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
            expected_file_sha256: request.payload_sha256().to_owned(),
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
            || !lower_hex_id(&self.expected_file_sha256)
            || self.runner_directory.target
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

    pub(in crate::launch::resource_probe::publication) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(in crate::launch::resource_probe::publication) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(in crate::launch::resource_probe::publication) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(in crate::launch::resource_probe::publication) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(in crate::launch::resource_probe::publication) fn projection_digest(&self) -> &str {
        &self.projection_digest
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_mountpoint(
        &self,
    ) -> &str {
        &self.metadata_volume_mountpoint
    }

    pub(in crate::launch::resource_probe::publication) fn expected_file_sha256(&self) -> &str {
        &self.expected_file_sha256
    }

    pub(in crate::launch::resource_probe::publication) fn runner_directory(
        &self,
    ) -> &ExecutionChannelDirectory {
        &self.runner_directory
    }

    pub(in crate::launch::resource_probe::publication) fn matches_record(
        &self,
        record: &crate::journal::ExecutionPublicationRecoveryRecord,
    ) -> bool {
        self.operation_id == record.operation_id
            && self.instance_id == record.instance_id
            && self.engine_id == record.engine_id
            && self.docker_root_digest == record.docker_root_digest
            && self.source_revision == record.source_revision
            && self.runtime_image_id == record.runtime_image_id
            && self.image_binding_digest == record.image_binding_digest
            && self.projection_digest == record.projection_digest
            && self.parent_kind == record.parent_kind
            && self.parent_operation_id == record.parent_operation_id
            && self.reservation_id == record.reservation_id
            && self.connection_id == record.connection_id
            && self.runner_id == record.runner_id
            && self.runner_container_id == record.runner_container_id
            && self.dind_container_id == record.dind_container_id
            && self.metadata_volume_name == record.metadata_volume_name
            && self.metadata_volume_id == record.metadata_volume_id
            && self.metadata_layout_version == record.metadata_layout_version
            && self.metadata_layout_digest == record.metadata_layout_digest
            && self.guest_boot_id == record.guest_boot_id
            && self.expected_file_sha256 == record.payload_sha256
    }

    pub(in crate::launch::resource_probe::publication) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(in crate::launch::resource_probe::publication) fn runtime_image_id(&self) -> &str {
        &self.runtime_image_id
    }

    pub(in crate::launch::resource_probe::publication) fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub(in crate::launch::resource_probe::publication) fn image_binding_digest(&self) -> &str {
        &self.image_binding_digest
    }

    pub(in crate::launch::resource_probe::publication) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_operation_id(&self) -> String {
        self.parent_operation_id.to_string()
    }
}

fn lower_hex_string(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_mount_root(value: &str) -> bool {
    value.starts_with('/')
        && !value.chars().any(char::is_control)
        && (value == "/"
            || (!value.ends_with('/')
                && !value.contains("//")
                && value
                    .split('/')
                    .skip(1)
                    .all(|part| !part.is_empty() && part != "." && part != "..")))
}

fn valid_mountpoint(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && !value.contains("//")
        && value
            .split('/')
            .skip(1)
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}

fn lower_hex_id(value: &str) -> bool {
    (value.len() == 32 || value.len() == 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{PublicationRecoveryProjection, lower_hex};
    use sha2::{Digest, Sha256};
    use velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory;

    fn recovery() -> PublicationRecoveryProjection {
        let operation_id = "0123456789abcdef0123456789abcdef".to_owned();
        let image = format!("sha256:{}", "a".repeat(64));
        let name = format!("velnor-execution-publication-{operation_id}");
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(name.clone()),
            platform: "linux/amd64".to_owned(),
        };
        let config = bollard::models::ContainerCreateBody {
            image: Some(image.clone()),
            ..Default::default()
        };
        let projection_digest = lower_hex(&Sha256::digest(
            crate::launch::resource_probe::projection::canonical::pair(&options, &config)
                .expect("projection"),
        ));
        let root = super::super::DockerRoot::parse("/var/lib/docker").expect("Docker root");
        PublicationRecoveryProjection {
            schema_version: 1,
            operation_id,
            instance_id: "b".repeat(32),
            engine_id: "selected-engine".to_owned(),
            docker_root_digest: root.digest().to_owned(),
            operation_name: name,
            platform: "linux/amd64".to_owned(),
            source_revision: "c".repeat(40),
            runtime_image_id: image,
            image_binding_digest: "d".repeat(64),
            projection_digest,
            metadata_volume_name: "metadata-owned".to_owned(),
            metadata_volume_id: "metadata-owned".to_owned(),
            metadata_volume_mountpoint: "/var/lib/docker/volumes/metadata-owned/_data".to_owned(),
            expected_file_sha256: "e".repeat(64),
            parent_kind: "challenge".to_owned(),
            parent_operation_id: 1,
            reservation_id: 2,
            connection_id: "private-connection".to_owned(),
            runner_id: 3,
            runner_container_id: "1".repeat(64),
            dind_container_id: "2".repeat(64),
            metadata_layout_version: 2,
            metadata_layout_digest: "f".repeat(64),
            guest_boot_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
            publisher_mode: "execution_publication".to_owned(),
            runner_directory: ExecutionChannelDirectory {
                target: "/opt/velnor/source-proof/control".to_owned(),
                inode: 11,
                owner_uid: 0,
                owner_gid: 0,
                mode: 0o555,
                mount_id: 12,
                statx_mount_id: 12,
                mount_root: "/source-proof/control".to_owned(),
                mount_source: "/dev/vda".to_owned(),
                mount_device_major: 8,
                mount_device_minor: 0,
                statx_device_major: 8,
                statx_device_minor: 0,
                filesystem_type_magic: 0xef53,
                filesystem_type_name: "ext4".to_owned(),
                filesystem_id: 13,
                read_only: true,
                executable_mount: false,
            },
            config,
            expected_config: bollard::models::ContainerConfig::default(),
        }
    }

    #[test]
    fn recovery_projection_is_canonical_bounded_and_tamper_evident() {
        let value = recovery();
        let encoded = value.encode().expect("bounded projection");
        let sha = lower_hex(&Sha256::digest(encoded.as_bytes()));
        let decoded = PublicationRecoveryProjection::decode(&encoded, &sha)
            .expect("same projection after restart");
        assert_eq!(decoded.encode().expect("canonical bytes"), encoded);
        assert!(PublicationRecoveryProjection::decode(&encoded, &"0".repeat(64)).is_err());

        let tampered = encoded.replace("metadata-owned", "foreign-volume");
        assert!(PublicationRecoveryProjection::decode(&tampered, &sha).is_err());
    }

    #[test]
    fn recovery_projection_rejects_bad_volume_and_parent_facts() {
        let mut value = recovery();
        value.metadata_volume_id = "foreign-volume".to_owned();
        assert!(value.encode().is_err());

        let mut value = recovery();
        value.dind_container_id = value.runner_container_id.clone();
        assert!(value.encode().is_err());
    }

    #[test]
    fn job_facts_recovery_keeps_fixed_writer_mode_and_mount() {
        let mut value = recovery();
        value.parent_kind = "job-facts".to_owned();
        value.publisher_mode = "job_facts_publication".to_owned();
        value.runner_directory.target = "/opt/velnor/job-facts".to_owned();
        value.runner_directory.mount_root = "/job-facts".to_owned();
        value.config.cmd = Some(vec!["--mode".to_owned(), value.publisher_mode.clone()]);
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(value.operation_name.clone()),
            platform: value.platform.clone(),
        };
        value.projection_digest = lower_hex(&Sha256::digest(
            crate::launch::resource_probe::projection::canonical::pair(&options, &value.config)
                .expect("job facts projection bytes"),
        ));

        let encoded = value.encode().expect("valid JobFacts recovery projection");
        let sha = lower_hex(&Sha256::digest(encoded.as_bytes()));
        let restored = PublicationRecoveryProjection::decode(&encoded, &sha)
            .expect("JobFacts recovery projection after restart");
        let projected = restored
            .into_projection("/var/lib/docker")
            .expect("fixed JobFacts publisher projection");
        assert_eq!(projected.publisher_mode, "job_facts_publication");
        assert!(projected.request.is_none());
    }

    #[test]
    fn recovery_bytes_are_stable_for_fresh_multilabel_maps() {
        let mut expected = None;
        for order in [
            ["runner", "operation", "instance", "revision"],
            ["revision", "instance", "operation", "runner"],
            ["operation", "runner", "revision", "instance"],
        ] {
            let values = [
                ("runner", "velnor".to_owned()),
                ("operation", "execution-publication".to_owned()),
                ("instance", "a".repeat(32)),
                ("revision", "c".repeat(40)),
            ];
            let mut labels = std::collections::HashMap::new();
            for key in order {
                let value = &values.iter().find(|(label, _)| *label == key).unwrap().1;
                labels.insert(key.to_owned(), value.clone());
            }
            let mut value = recovery();
            value.config.labels = Some(labels.clone());
            value.expected_config.labels = Some(labels);
            let options = bollard::query_parameters::CreateContainerOptions {
                name: Some(value.operation_name.clone()),
                platform: value.platform.clone(),
            };
            value.projection_digest = lower_hex(&Sha256::digest(
                crate::launch::resource_probe::projection::canonical::pair(&options, &value.config)
                    .expect("projection digest bytes"),
            ));
            let bytes = value.encode().expect("canonical recovery projection");
            if let Some(expected) = &expected {
                assert_eq!(&bytes, expected);
            } else {
                expected = Some(bytes);
            }
        }
    }
}
