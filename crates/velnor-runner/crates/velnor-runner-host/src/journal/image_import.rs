//! Durable identity and status for host image imports.

mod storage;
#[cfg(test)]
mod tests;

use crate::error::HostError;

use super::Journal;

/// Exact verified artifact and selected daemon identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportRequest {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    archive_sha256: String,
    index_digest: String,
    platform: String,
    manifest_digest: String,
    config_digest: String,
}

/// Immutable daemon and artifact values supplied for one import operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportIdentity {
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) daemon_architecture: String,
    pub(crate) source_sha: String,
    pub(crate) archive_sha256: String,
    pub(crate) index_digest: String,
    pub(crate) platform: String,
    pub(crate) manifest_digest: String,
    pub(crate) config_digest: String,
}

impl ImageImportRequest {
    /// Build a request from verified release and daemon identities.
    pub(crate) fn new(identity: ImageImportIdentity) -> Result<Self, HostError> {
        let request = Self {
            engine_id: identity.engine_id,
            docker_root_digest: identity.docker_root_digest,
            daemon_architecture: identity.daemon_architecture,
            source_sha: identity.source_sha,
            archive_sha256: identity.archive_sha256,
            index_digest: identity.index_digest,
            platform: identity.platform,
            manifest_digest: identity.manifest_digest,
            config_digest: identity.config_digest,
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

    fn validate(&self) -> Result<(), HostError> {
        let valid_engine = !self.engine_id.is_empty()
            && self.engine_id.len() <= 128
            && self
                .engine_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"'));
        if !valid_engine
            || !valid_hex(&self.docker_root_digest, 64)
            || !matches!(self.daemon_architecture.as_str(), "x86_64" | "aarch64")
            || !valid_hex(&self.source_sha, 40)
            || !valid_hex(&self.archive_sha256, 64)
            || !valid_hex(&self.index_digest, 64)
            || self.platform != "linux/amd64"
            || !valid_digest(&self.manifest_digest)
            || !valid_digest(&self.config_digest)
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }
}

/// Verified selected-daemon image inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportInspection {
    request: ImageImportRequest,
    image_id: String,
}

impl ImageImportInspection {
    /// Bind a completed inspection to the exact request.
    pub(crate) fn verified(
        request: &ImageImportRequest,
        observed_engine_id: &str,
        observed_docker_root_digest: &str,
        observed_daemon_architecture: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        request.validate()?;
        if request.engine_id != observed_engine_id
            || request.docker_root_digest != observed_docker_root_digest
            || request.daemon_architecture != observed_daemon_architecture
            || !valid_digest(image_id)
            || (image_id != request.config_digest && image_id != request.manifest_digest)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            request: request.clone(),
            image_id: image_id.to_owned(),
        })
    }
}

/// Durable image-import lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageImportState {
    /// Intent exists; no import effect was claimed.
    Prepared,
    /// One-shot Docker import claim committed; effect may be in progress.
    Requested,
    /// Docker import response was ambiguous.
    Uncertain,
    /// Exact image was verified on the selected daemon.
    Completed,
    /// Intent was safely abandoned before any effect claim.
    Aborted,
}

impl ImageImportState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Requested => "requested",
            Self::Uncertain => "uncertain",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "completed" => Ok(Self::Completed),
            "aborted" => Ok(Self::Aborted),
            _ => Err(HostError::Journal),
        }
    }
}

/// Stable row identity for one import attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageImportId(i64);

/// In-memory authority for the one live import request claimed by this caller.
#[derive(Debug)]
pub(crate) struct ImageImportClaim {
    journal: Journal,
    id: ImageImportId,
    request: ImageImportRequest,
}

impl ImageImportClaim {
    pub(super) fn new(journal: &Journal, id: ImageImportId, request: &ImageImportRequest) -> Self {
        Self {
            journal: journal.clone(),
            id,
            request: request.clone(),
        }
    }
}

/// Durable import attempt returned by prepare and read operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageImportRecord {
    pub(crate) id: ImageImportId,
    pub(crate) attempt: i64,
    pub(crate) state: ImageImportState,
    pub(crate) request: ImageImportRequest,
    pub(crate) image_id: Option<String>,
}

impl Journal {
    /// Prepare an exact import or replay its latest durable attempt.
    pub(crate) async fn prepare_image_import(
        &self,
        request: ImageImportRequest,
    ) -> Result<ImageImportRecord, HostError> {
        storage::prepare(self, request).await
    }

    /// Claim the import effect once for the exact request and selected engine.
    pub(crate) async fn claim_image_import_effect(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
    ) -> Result<Option<ImageImportClaim>, HostError> {
        storage::claim(self, id, request).await
    }

    /// Mark a post-claim Docker result uncertain; uncertainty cannot be retried.
    pub(crate) async fn mark_image_import_uncertain(
        &self,
        claim: ImageImportClaim,
    ) -> Result<(), HostError> {
        storage::mark_uncertain(self, claim).await
    }

    /// Complete the claimed load after its stream succeeds and inspection matches.
    pub(crate) async fn complete_claimed_image_import(
        &self,
        claim: ImageImportClaim,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_claimed(self, claim, inspection).await
    }

    /// Complete a verified preloaded image only before any import claim.
    pub(crate) async fn complete_preloaded_image_import(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
        inspection: &ImageImportInspection,
    ) -> Result<(), HostError> {
        storage::complete_preloaded(self, id, request, inspection).await
    }

    /// Abandon only an intent whose one-shot effect was never claimed.
    pub(crate) async fn abort_image_import_before_effect(
        &self,
        id: ImageImportId,
        request: &ImageImportRequest,
    ) -> Result<(), HostError> {
        storage::abort_before_effect(self, id, request).await
    }
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| valid_hex(digest, 64))
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
