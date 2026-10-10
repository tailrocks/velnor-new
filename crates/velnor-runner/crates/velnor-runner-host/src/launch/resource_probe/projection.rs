//! Narrow Docker projection for the controller-owned guest probe.

use std::fmt;

use bollard::models::{
    ContainerConfig, ContainerCreateBody, HostConfig, Mount, MountBindOptions, MountType,
};
use bollard::query_parameters::CreateContainerOptions;
use sha2::{Digest, Sha256};

use crate::error::HostError;

#[path = "projection/canonical.rs"]
mod canonical;

const PROBE_TARGET: &str = "/velnor/docker-root";
const PROBE_PLATFORM: &str = "linux/amd64";
const PROBE_ENTRYPOINT: &str = "/velnor/resource-probe";
const PROBE_USER: &str = "65532:65532";
const PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
const DIGEST_DOMAIN: &[u8] = b"velnor-resource-probe-root-v1\0";

/// Daemon-specific immutable image reference returned by the future verifier.
pub(super) struct VerifiedProbeImage {
    runtime_id: String,
    platform: String,
    source_revision: String,
    binding_fingerprint: String,
}

impl fmt::Debug for VerifiedProbeImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedProbeImage")
            .field("runtime_id", &self.runtime_id)
            .field("platform", &self.platform)
            .field("source_revision", &self.source_revision)
            .field("binding_fingerprint", &self.binding_fingerprint)
            .finish()
    }
}

impl VerifiedProbeImage {
    /// Accept only the opaque immutable identity produced by the verifier.
    pub(super) fn from_verified_provider(
        runtime_id: String,
        platform: String,
        source_revision: String,
        binding_fingerprint: String,
        config: &ContainerConfig,
    ) -> Result<Self, HostError> {
        if !digest_reference(&runtime_id)
            || platform != PROBE_PLATFORM
            || !hex_string(&source_revision, 40)
            || !hex_string(&binding_fingerprint, 64)
            || !image_config_matches(config, &source_revision)
        {
            return Err(HostError::Docker);
        }
        Ok(Self {
            runtime_id,
            platform,
            source_revision,
            binding_fingerprint,
        })
    }

    pub(super) fn from_journal(
        runtime_id: String,
        source_revision: String,
        binding_fingerprint: String,
    ) -> Result<Self, HostError> {
        if !digest_reference(&runtime_id)
            || !hex_string(&source_revision, 40)
            || !hex_string(&binding_fingerprint, 64)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id,
            platform: PROBE_PLATFORM.to_owned(),
            source_revision,
            binding_fingerprint,
        })
    }

    pub(super) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(super) fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub(super) fn binding_fingerprint(&self) -> &str {
        &self.binding_fingerprint
    }

    #[cfg(test)]
    pub(super) fn test_fixture() -> Self {
        let id = format!("sha256:{}", "a".repeat(64));
        let revision = "b".repeat(40);
        let fingerprint = "c".repeat(64);
        Self {
            runtime_id: id,
            platform: PROBE_PLATFORM.to_owned(),
            source_revision: revision,
            binding_fingerprint: fingerprint,
        }
    }
}

/// Canonical guest path, intentionally redacted from diagnostics.
pub(super) struct DockerRoot {
    path: String,
    digest: String,
}

impl fmt::Debug for DockerRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockerRoot")
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

impl DockerRoot {
    /// Reject noncanonical, relative, control-bearing, and root paths.
    pub(super) fn parse(path: &str) -> Result<Self, HostError> {
        if !canonical_absolute_nonroot(path) {
            return Err(HostError::Path);
        }
        let mut hasher = Sha256::new();
        hasher.update(DIGEST_DOMAIN);
        hasher.update(path.as_bytes());
        let digest = digest_text(&hasher.finalize());
        Ok(Self {
            path: path.to_owned(),
            digest,
        })
    }

    pub(super) fn digest(&self) -> &str {
        &self.digest
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }
}

/// Complete create request. Its fields are inspected again before cleanup.
pub(super) struct ProbeProjection {
    pub(super) name: String,
    pub(super) operation_id: String,
    pub(super) root: DockerRoot,
    pub(super) image: VerifiedProbeImage,
    pub(super) engine_id: String,
    pub(super) instance_id: String,
    pub(super) options: CreateContainerOptions,
    pub(super) config: ContainerCreateBody,
    pub(super) projection_digest: String,
}

impl fmt::Debug for ProbeProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProbeProjection")
            .field("name", &self.name)
            .field("operation_id", &self.operation_id)
            .field("root", &self.root)
            .field("image", &self.image)
            .field("engine_id", &self.engine_id)
            .field("instance_id", &self.instance_id)
            .field("projection_digest", &self.projection_digest)
            .finish_non_exhaustive()
    }
}

impl ProbeProjection {
    /// Construct the fixed private projection from verified daemon and image inputs.
    pub(super) fn build(
        operation_id: String,
        instance_id: String,
        engine_id: String,
        root_path: &str,
        image: VerifiedProbeImage,
    ) -> Result<Self, HostError> {
        if !hex_string(&operation_id, 32)
            || !hex_string(&instance_id, 32)
            || engine_id.is_empty()
            || engine_id.len() > 256
            || engine_id.chars().any(char::is_control)
        {
            return Err(HostError::Identity);
        }
        let root = DockerRoot::parse(root_path)?;
        let name = format!("velnor-resource-probe-{operation_id}");
        let config = config(&root, &image, &operation_id, &instance_id, &engine_id);
        let options = CreateContainerOptions {
            name: Some(name.clone()),
            platform: image.platform.clone(),
        };
        let digest = projection_digest(&options, &config)?;
        Ok(Self {
            name,
            operation_id,
            root,
            image,
            engine_id,
            instance_id,
            options,
            config,
            projection_digest: digest,
        })
    }
}

fn config(
    root: &DockerRoot,
    image: &VerifiedProbeImage,
    operation_id: &str,
    instance_id: &str,
    engine_id: &str,
) -> ContainerCreateBody {
    let engine_digest = digest_text(engine_id.as_bytes());
    let mut labels = std::collections::HashMap::new();
    labels.insert("velnor.role".to_owned(), "resource-probe".to_owned());
    labels.insert("velnor.operation".to_owned(), operation_id.to_owned());
    labels.insert("velnor.instance".to_owned(), instance_id.to_owned());
    labels.insert("velnor.engine".to_owned(), engine_digest);
    labels.insert(
        "velnor.probe.binding".to_owned(),
        image.binding_fingerprint.clone(),
    );
    labels.insert(SOURCE_LABEL.to_owned(), image.source_revision.clone());
    ContainerCreateBody {
        image: Some(image.runtime_id.clone()),
        user: Some(PROBE_USER.to_owned()),
        env: None,
        cmd: None,
        entrypoint: None,
        working_dir: None,
        labels: Some(labels),
        attach_stdin: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        network_disabled: Some(true),
        host_config: Some(host_config(root)),
        ..Default::default()
    }
}

fn host_config(root: &DockerRoot) -> HostConfig {
    HostConfig {
        auto_remove: Some(false),
        cap_drop: Some(vec!["ALL".to_owned()]),
        mounts: Some(vec![Mount {
            target: Some(PROBE_TARGET.to_owned()),
            source: Some(root.path.clone()),
            typ: Some(MountType::BIND),
            read_only: Some(true),
            bind_options: Some(MountBindOptions {
                create_mountpoint: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        network_mode: Some("none".to_owned()),
        privileged: Some(false),
        readonly_rootfs: Some(true),
        security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
        ..Default::default()
    }
}

fn projection_digest(
    options: &CreateContainerOptions,
    config: &ContainerCreateBody,
) -> Result<String, HostError> {
    canonical::pair(options, config).map(|bytes| digest_text(&bytes))
}

#[cfg(test)]
pub(super) fn test_projection_digest(
    options: &CreateContainerOptions,
    config: &ContainerCreateBody,
) -> Result<String, HostError> {
    projection_digest(options, config)
}

fn canonical_absolute_nonroot(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && path[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn digest_reference(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex_string(hex, 64))
}

fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_text(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn image_config_matches(config: &ContainerConfig, source_revision: &str) -> bool {
    config == &expected_image_config(source_revision)
}

pub(super) fn expected_image_config(source_revision: &str) -> ContainerConfig {
    let mut labels = std::collections::HashMap::new();
    labels.insert(SOURCE_LABEL.to_owned(), source_revision.to_owned());
    ContainerConfig {
        user: Some(PROBE_USER.to_owned()),
        env: Some(vec![PATH_ENV.to_owned()]),
        working_dir: Some("/".to_owned()),
        entrypoint: Some(vec![PROBE_ENTRYPOINT.to_owned()]),
        labels: Some(labels),
        ..Default::default()
    }
}
