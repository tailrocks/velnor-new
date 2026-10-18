//! Original release identity and exact Docker load transport identity.

use crate::error::HostError;

/// Original image, selected daemon, and optional derived load archive identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportIdentity {
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) daemon_architecture: String,
    pub(crate) source_sha: String,
    pub(crate) archive_sha256: String,
    pub(crate) archive_index_digest: String,
    pub(crate) platform: String,
    pub(crate) manifest_digest: String,
    pub(crate) manifest_size: u64,
    pub(crate) config_digest: String,
    pub(crate) image_index_digest: Option<String>,
    pub(crate) image_index_size: Option<u64>,
    pub(crate) load_archive_sha256: Option<String>,
    pub(crate) load_archive_index_digest: Option<String>,
    pub(crate) image_reference: String,
}

/// Exact immutable request persisted before a Docker image load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportRequest {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) daemon_architecture: String,
    pub(super) source_sha: String,
    pub(super) archive_sha256: String,
    pub(super) archive_index_digest: String,
    pub(super) platform: String,
    pub(super) manifest_digest: String,
    pub(super) manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) image_index_digest: Option<String>,
    pub(super) image_index_size: Option<u64>,
    pub(super) load_archive_sha256: Option<String>,
    pub(super) load_archive_index_digest: Option<String>,
    pub(super) image_reference: String,
}

impl ImageImportRequest {
    /// Make a validated immutable import request.
    pub(crate) fn new(identity: ImageImportIdentity) -> Result<Self, HostError> {
        let request = Self {
            engine_id: identity.engine_id,
            docker_root_digest: identity.docker_root_digest,
            daemon_architecture: identity.daemon_architecture,
            source_sha: identity.source_sha,
            archive_sha256: identity.archive_sha256,
            archive_index_digest: identity.archive_index_digest,
            platform: identity.platform,
            manifest_digest: identity.manifest_digest,
            manifest_size: identity.manifest_size,
            config_digest: identity.config_digest,
            image_index_digest: identity.image_index_digest,
            image_index_size: identity.image_index_size,
            load_archive_sha256: identity.load_archive_sha256,
            load_archive_index_digest: identity.load_archive_index_digest,
            image_reference: identity.image_reference,
        };
        request.validate()?;
        Ok(request)
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }

    pub(super) fn image_id_digest(&self) -> &str {
        self.image_index_digest
            .as_deref()
            .unwrap_or(&self.manifest_digest)
    }

    pub(super) fn image_descriptor_size(&self) -> u64 {
        self.image_index_size.unwrap_or(self.manifest_size)
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        let valid_engine = !self.engine_id.is_empty()
            && self.engine_id.len() <= 128
            && self
                .engine_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"'));
        let has_nested_index = self.image_index_digest.is_some();
        let has_index_size = self.image_index_size.is_some();
        let load_identity_matches =
            self.load_archive_sha256.is_some() == self.load_archive_index_digest.is_some();
        let has_load = self.load_archive_sha256.is_some();
        let nested_digest_ok = self.image_index_digest.as_deref().is_none_or(valid_digest);
        let load_hashes_ok = self
            .load_archive_sha256
            .as_deref()
            .is_none_or(|value| valid_hex(value, 64))
            && self
                .load_archive_index_digest
                .as_deref()
                .is_none_or(|value| valid_hex(value, 64));
        let derived_reference_ok = !has_load || self.is_canonical_image_reference();
        if !valid_engine
            || !valid_hex(&self.docker_root_digest, 64)
            || !matches!(self.daemon_architecture.as_str(), "x86_64" | "aarch64")
            || !valid_hex(&self.source_sha, 40)
            || !valid_hex(&self.archive_sha256, 64)
            || !valid_hex(&self.archive_index_digest, 64)
            || self.platform != "linux/amd64"
            || !valid_digest(&self.manifest_digest)
            || self.manifest_size == 0
            || self.manifest_size > i64::MAX as u64
            || !valid_digest(&self.config_digest)
            || has_nested_index != has_index_size
            || self.image_index_size == Some(0)
            || self
                .image_index_size
                .is_some_and(|size| size > i64::MAX as u64)
            || !nested_digest_ok
            || !load_identity_matches
            || !load_hashes_ok
            || !valid_image_reference(&self.image_reference)
            || !derived_reference_ok
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    fn is_canonical_image_reference(&self) -> bool {
        let Some((name, tag)) = self.image_reference.split_once(':') else {
            return false;
        };
        matches!(
            name,
            "velnor-runner" | "velnor-dind" | "velnor-resource-probe"
        ) && tag == format!("v{}-{}", self.source_sha, self.archive_sha256)
    }
}

pub(super) fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| valid_hex(digest, 64))
}

pub(super) fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_image_reference(value: &str) -> bool {
    let Some((name, tag)) = value.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && !tag.is_empty()
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
