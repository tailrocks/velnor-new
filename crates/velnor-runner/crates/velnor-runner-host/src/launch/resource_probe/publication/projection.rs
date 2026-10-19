//! Fixed Docker projection for one control-file publisher.

use bollard::models::{
    ContainerConfig, ContainerCreateBody, HostConfig, HostConfigCgroupnsModeEnum, Mount, MountType,
    MountVolumeOptions,
};
use bollard::query_parameters::CreateContainerOptions;
use sha2::{Digest, Sha256};
use velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory;

use crate::error::HostError;

use super::super::projection::DockerRoot;
use super::super::projection::VerifiedProbeImage;
use super::request::PublicationRequest;

mod recovery;
pub(super) use recovery::PublicationRecoveryProjection;

const PLATFORM: &str = "linux/amd64";
const PUBLISHER_PATH: &str = "/velnor/resource-probe";
const METADATA_TARGET: &str = "/opt/velnor";
const CPU_PERIOD: i64 = 100_000;
const CPU_QUOTA: i64 = 10_000;
const MEMORY_BYTES: i64 = 128 * 1024 * 1024;
const PID_LIMIT: i64 = 4;
const ROOT_USER: &str = "0:0";
const MAX_CONTROL_PAYLOAD_BYTES: usize =
    velnor_resource_probe::execution_publication::PUBLICATION_REQUEST_LIMIT;

pub(super) struct PublicationProjection {
    pub(super) name: String,
    pub(super) operation_id: String,
    pub(super) root: DockerRoot,
    pub(super) engine_id: String,
    pub(super) metadata_volume: String,
    pub(super) options: CreateContainerOptions,
    pub(super) config: ContainerCreateBody,
    pub(super) expected_config: ContainerConfig,
    pub(super) digest: String,
    pub(super) request: Option<PublicationRequest>,
    pub(super) publisher_mode: String,
    pub(super) instance_id: String,
    pub(super) source_revision: String,
    pub(super) image_binding_digest: String,
}

impl PublicationProjection {
    pub(super) fn request_image(&self) -> &str {
        self.config.image.as_deref().unwrap_or_default()
    }

    pub(super) fn build(
        operation_id: String,
        instance_id: &str,
        engine_id: &str,
        root_path: &str,
        metadata_volume: &str,
        image: &VerifiedProbeImage,
        request: impl Into<PublicationRequest>,
    ) -> Result<Self, HostError> {
        let request = request.into();
        request.validate()?;
        if !lower_hex(&operation_id, 32)
            || !lower_hex(instance_id, 32)
            || engine_id.is_empty()
            || engine_id.len() > 256
            || engine_id.chars().any(char::is_control)
            || !valid_volume(metadata_volume)
            || request.operation_id() != operation_id
            || !lower_hex(request.payload_sha256(), 64)
            || !valid_boot_id(request.guest_boot_id())
            || request.directory().inode == 0
            || request.directory().mount_source.is_empty()
        {
            return Err(HostError::Identity);
        }
        let root = DockerRoot::parse(root_path)?;
        if image.platform() != PLATFORM {
            return Err(HostError::Identity);
        }
        let name = format!("velnor-execution-publication-{operation_id}");
        let options = CreateContainerOptions {
            name: Some(name.clone()),
            platform: PLATFORM.to_owned(),
        };
        let publisher_mode = request.mode().to_owned();
        let config = container_config(
            &operation_id,
            instance_id,
            engine_id,
            metadata_volume,
            image,
            &publisher_mode,
        );
        let expected_config = effective_config(image.source_revision(), &config)?;
        let canonical =
            crate::launch::resource_probe::projection::canonical::pair(&options, &config)?;
        let projection_digest = digest(&canonical);
        Ok(Self {
            name,
            operation_id,
            root,
            engine_id: engine_id.to_owned(),
            metadata_volume: metadata_volume.to_owned(),
            options,
            config,
            expected_config,
            digest: projection_digest,
            request: Some(request),
            publisher_mode,
            instance_id: instance_id.to_owned(),
            source_revision: image.source_revision().to_owned(),
            image_binding_digest: image.binding_fingerprint().to_owned(),
        })
    }
}

fn container_config(
    operation_id: &str,
    instance_id: &str,
    engine_id: &str,
    metadata_volume: &str,
    image: &VerifiedProbeImage,
    publisher_mode: &str,
) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(image.runtime_id().to_owned()),
        user: Some(ROOT_USER.to_owned()),
        entrypoint: Some(vec![PUBLISHER_PATH.to_owned()]),
        cmd: Some(vec!["--mode".to_owned(), publisher_mode.to_owned()]),
        attach_stdin: Some(true),
        open_stdin: Some(true),
        stdin_once: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        network_disabled: Some(true),
        labels: Some(std::collections::HashMap::from([
            ("velnor.role".to_owned(), "execution-publication".to_owned()),
            ("velnor.operation".to_owned(), operation_id.to_owned()),
            ("velnor.instance".to_owned(), instance_id.to_owned()),
            ("velnor.engine".to_owned(), digest(engine_id.as_bytes())),
            (
                "velnor.probe.binding".to_owned(),
                image.binding_fingerprint().to_owned(),
            ),
            (
                "org.opencontainers.image.revision".to_owned(),
                image.source_revision().to_owned(),
            ),
        ])),
        host_config: Some(HostConfig {
            auto_remove: Some(false),
            cap_drop: Some(vec!["ALL".to_owned()]),
            cap_add: Some(vec!["DAC_OVERRIDE".to_owned()]),
            cgroupns_mode: Some(HostConfigCgroupnsModeEnum::PRIVATE),
            cpu_period: Some(CPU_PERIOD),
            cpu_quota: Some(CPU_QUOTA),
            memory: Some(MEMORY_BYTES),
            memory_swap: Some(MEMORY_BYTES),
            pids_limit: Some(PID_LIMIT),
            privileged: Some(false),
            readonly_rootfs: Some(true),
            network_mode: Some("none".to_owned()),
            security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
            mounts: Some(vec![Mount {
                target: Some(METADATA_TARGET.to_owned()),
                source: Some(metadata_volume.to_owned()),
                typ: Some(MountType::VOLUME),
                read_only: Some(false),
                volume_options: Some(MountVolumeOptions {
                    no_copy: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn effective_config(
    source_revision: &str,
    request: &ContainerCreateBody,
) -> Result<ContainerConfig, HostError> {
    let mut expected = serde_json::to_value(super::super::projection::expected_image_config(
        source_revision,
    ))
    .map_err(|_| HostError::Docker)?;
    let requested = serde_json::to_value(request).map_err(|_| HostError::Docker)?;
    let expected = expected.as_object_mut().ok_or(HostError::Docker)?;
    let requested = requested.as_object().ok_or(HostError::Docker)?;
    for (key, value) in requested {
        if value.is_null() || key == "HostConfig" || key == "NetworkingConfig" {
            continue;
        }
        if key == "Labels" {
            let expected_labels = expected
                .get_mut(key)
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(HostError::Docker)?;
            let requested_labels = value.as_object().ok_or(HostError::Docker)?;
            for (label, label_value) in requested_labels {
                expected_labels.insert(label.clone(), label_value.clone());
            }
        } else {
            expected.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(serde_json::Value::Object(expected.clone()))
        .map_err(|_| HostError::Docker)
}

pub(super) fn request(
    operation_id: String,
    guest_boot_id: &str,
    kind: velnor_resource_probe::execution_publication::ExecutionPublicationKind,
    payload: &[u8],
    payload_sha256: &str,
    runner_control: &ExecutionChannelDirectory,
) -> Result<velnor_resource_probe::execution_publication::ExecutionPublicationContext, HostError> {
    if payload.is_empty()
        || payload.len() > MAX_CONTROL_PAYLOAD_BYTES
        || digest(payload) != payload_sha256
        || !valid_boot_id(guest_boot_id)
    {
        return Err(HostError::Identity);
    }
    let request = velnor_resource_probe::execution_publication::ExecutionPublicationContext {
        schema_version: 1,
        operation_id,
        expected_guest_boot_id: guest_boot_id.to_owned(),
        kind,
        payload_hex: encode_hex(payload),
        payload_sha256: payload_sha256.to_owned(),
        expected_runner_control: runner_control.clone(),
    };
    let encoded = serde_json::to_vec(&request).map_err(|_| HostError::Docker)?;
    if encoded.len() > velnor_resource_probe::execution_publication::PUBLICATION_REQUEST_LIMIT {
        return Err(HostError::Frame);
    }
    Ok(request)
}

fn valid_volume(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
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

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest(value: &[u8]) -> String {
    encode_hex(&Sha256::digest(value))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
