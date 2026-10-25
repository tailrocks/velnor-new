//! Narrow Docker projection for the controller-owned guest probe.

use std::fmt;

use bollard::models::{ContainerConfig, ContainerCreateBody};
use bollard::query_parameters::CreateContainerOptions;
use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::ResourceProbePurpose;
use crate::worker::{AdmissionReserve, PROBE_CPU_MILLICORES, PROBE_MEMORY_LIMIT_BYTES};

#[path = "projection/canonical.rs"]
pub(super) mod canonical;
#[path = "projection/container_config.rs"]
mod container_config;
mod validation;

use validation::{
    canonical_absolute_nonroot, digest_reference, digest_text, hex_string, image_config_matches,
    valid_daemon_architecture,
};
pub(super) use validation::{expected_container_config, expected_image_config};

const PROBE_TARGET: &str = "/velnor/docker-root";
pub(super) const CGROUP_TARGET: &str = "/sys/fs/cgroup";
const PROBE_PLATFORM: &str = "linux/amd64";
const PROBE_ENTRYPOINT: &str = "/velnor/resource-probe";
pub(super) const PROBE_USER: &str = "65532:65532";
const PROBE_WORKING_DIR: &str = "/";
const PROBE_CPU_PERIOD: i64 = 100_000;
const PROBE_CPU_QUOTA: i64 = 10_000;
const PROBE_MEMORY_BYTES: i64 = PROBE_MEMORY_LIMIT_BYTES.cast_signed();
const PROBE_PIDS_LIMIT: i64 = 4;
const MILLICORES_PER_VCPU: u64 = 1_000;
const PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
const DAEMON_ARCH_LABEL: &str = "velnor.daemon.architecture";
const DIGEST_DOMAIN: &[u8] = b"velnor-resource-probe-root-v1\0";
const CAPACITY_REQUEST_DOMAIN: &[u8] = b"velnor-capacity-probe-request-v1\0";

const fn probe_cpu_millicores() -> u64 {
    (PROBE_CPU_QUOTA as u64 * MILLICORES_PER_VCPU) / PROBE_CPU_PERIOD as u64
}

const _: () = assert!(probe_cpu_millicores() == PROBE_CPU_MILLICORES);
const _: () = assert!(probe_cpu_millicores() <= AdmissionReserve::OPERATIONAL_CPU_MILLICORES);
const _: () = assert!(PROBE_MEMORY_BYTES > 0);

/// Daemon-specific immutable image reference returned by the future verifier.
pub(super) struct VerifiedProbeImage {
    runtime_id: String,
    /// Verified image platform. This remains `linux/amd64` under emulation.
    platform: String,
    /// Selected Docker engine architecture, separate from the image platform.
    daemon_architecture: String,
    source_revision: String,
    binding_fingerprint: String,
}

impl fmt::Debug for VerifiedProbeImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedProbeImage")
            .field("runtime_id", &self.runtime_id)
            .field("platform", &self.platform)
            .field("daemon_architecture", &self.daemon_architecture)
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
        daemon_architecture: String,
        source_revision: String,
        binding_fingerprint: String,
        config: &ContainerConfig,
    ) -> Result<Self, HostError> {
        if !digest_reference(&runtime_id)
            || platform != PROBE_PLATFORM
            || !valid_daemon_architecture(&daemon_architecture)
            || !hex_string(&source_revision, 40)
            || !hex_string(&binding_fingerprint, 64)
            || !image_config_matches(config, &source_revision)
        {
            return Err(HostError::Docker);
        }
        Ok(Self {
            runtime_id,
            platform,
            daemon_architecture,
            source_revision,
            binding_fingerprint,
        })
    }

    pub(super) fn from_journal(
        runtime_id: String,
        daemon_architecture: String,
        source_revision: String,
        binding_fingerprint: String,
    ) -> Result<Self, HostError> {
        if !digest_reference(&runtime_id)
            || !valid_daemon_architecture(&daemon_architecture)
            || !hex_string(&source_revision, 40)
            || !hex_string(&binding_fingerprint, 64)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id,
            platform: PROBE_PLATFORM.to_owned(),
            daemon_architecture,
            source_revision,
            binding_fingerprint,
        })
    }

    pub(super) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(super) fn platform(&self) -> &str {
        &self.platform
    }

    pub(super) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
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
            daemon_architecture: "x86_64".to_owned(),
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
    pub(super) purpose: ResourceProbePurpose,
    pub(super) request_sha256: String,
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
            .field("purpose", &self.purpose)
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
        Self::build_for_purpose(
            operation_id,
            instance_id,
            engine_id,
            root_path,
            image,
            ResourceProbePurpose::Capacity,
            capacity_request_sha256(),
        )
    }

    pub(super) fn build_owned_cgroups(
        operation_id: String,
        instance_id: String,
        engine_id: String,
        root_path: &str,
        image: VerifiedProbeImage,
        request_sha256: String,
    ) -> Result<Self, HostError> {
        Self::build_for_purpose(
            operation_id,
            instance_id,
            engine_id,
            root_path,
            image,
            ResourceProbePurpose::OwnedCgroups,
            request_sha256,
        )
    }

    fn build_for_purpose(
        operation_id: String,
        instance_id: String,
        engine_id: String,
        root_path: &str,
        image: VerifiedProbeImage,
        purpose: ResourceProbePurpose,
        request_sha256: String,
    ) -> Result<Self, HostError> {
        if !hex_string(&operation_id, 32)
            || !hex_string(&instance_id, 32)
            || !hex_string(&request_sha256, 64)
            || purpose == ResourceProbePurpose::Capacity
                && request_sha256 != capacity_request_sha256()
            || engine_id.is_empty()
            || engine_id.len() > 256
            || engine_id.chars().any(char::is_control)
        {
            return Err(HostError::Identity);
        }
        let root = DockerRoot::parse(root_path)?;
        let name = format!("velnor-resource-probe-{operation_id}");
        let config = container_config::config(
            &root,
            &image,
            &operation_id,
            &instance_id,
            &engine_id,
            purpose,
            &request_sha256,
        );
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
            purpose,
            request_sha256,
        })
    }

    pub(super) fn build_from_purpose(
        operation_id: String,
        instance_id: String,
        engine_id: String,
        root_path: &str,
        image: VerifiedProbeImage,
        purpose: ResourceProbePurpose,
        request_sha256: String,
    ) -> Result<Self, HostError> {
        Self::build_for_purpose(
            operation_id,
            instance_id,
            engine_id,
            root_path,
            image,
            purpose,
            request_sha256,
        )
    }
}

fn capacity_request_sha256() -> String {
    digest_text(CAPACITY_REQUEST_DOMAIN)
}

pub(super) const fn resource_limits() -> (i64, i64, i64, i64, i64) {
    (
        PROBE_CPU_PERIOD,
        PROBE_CPU_QUOTA,
        PROBE_MEMORY_BYTES,
        PROBE_MEMORY_BYTES,
        PROBE_PIDS_LIMIT,
    )
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
