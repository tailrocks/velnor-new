//! Canonical guest-facing image and Scale Set claims.

use serde::Serialize;

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::launch::{VerifiedImageIdentity, VerifiedWorkerImages};

const GUEST_PLATFORM: &str = "linux/amd64";
const SCHEMA: &str = "velnor.guest-image/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct GuestImageDocument {
    schema: &'static str,
    source_sha: String,
    guest_platform: &'static str,
    scale_set: GuestScaleSet,
    images: GuestImages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestImages {
    runner: GuestImageIdentity,
    dind: GuestImageIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestScaleSet {
    name: String,
    labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestImageIdentity {
    archive_sha256: String,
    archive_index_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_index_digest: Option<String>,
    manifest_digest: String,
    config_digest: String,
}

impl GuestImageDocument {
    pub(super) fn new(
        connection: &RepositoryConnection,
        images: &VerifiedWorkerImages,
    ) -> Result<Self, HostError> {
        Self::validate_images(images)?;
        let selector = &connection.scale_set;
        let canonical_selector =
            crate::config::ScaleSetSelector::try_new(selector.name(), selector.labels())?;
        if canonical_selector != *selector {
            return Err(HostError::ForbiddenMount);
        }
        Ok(Self {
            schema: SCHEMA,
            source_sha: images.source_sha().to_owned(),
            guest_platform: GUEST_PLATFORM,
            scale_set: GuestScaleSet {
                name: selector.name().to_owned(),
                labels: selector.labels().to_vec(),
            },
            images: GuestImages {
                runner: GuestImageIdentity::new(images.runner())?,
                dind: GuestImageIdentity::new(images.dind())?,
            },
        })
    }

    pub(super) fn validate_images(images: &VerifiedWorkerImages) -> Result<(), HostError> {
        if images.engine_id().is_empty()
            || !lower_hex(images.docker_root_digest(), 64)
            || images.daemon_architecture().is_empty()
            || images.platform() != GUEST_PLATFORM
            || !lower_hex(images.source_sha(), 40)
        {
            return Err(HostError::ForbiddenMount);
        }
        GuestImageIdentity::new(images.runner())?;
        GuestImageIdentity::new(images.dind())?;
        Ok(())
    }
}

impl GuestImageIdentity {
    fn new(image: &VerifiedImageIdentity) -> Result<Self, HostError> {
        if !sha256_digest(image.runtime_id())
            || !lower_hex(image.archive_sha256(), 64)
            || !lower_hex(image.archive_index_digest(), 64)
            || image
                .image_index_digest()
                .is_some_and(|digest| !sha256_digest(digest))
            || !sha256_digest(image.manifest_digest())
            || !sha256_digest(image.config_digest())
        {
            return Err(HostError::ForbiddenMount);
        }
        Ok(Self {
            archive_sha256: image.archive_sha256().to_owned(),
            archive_index_digest: image.archive_index_digest().to_owned(),
            image_index_digest: image.image_index_digest().map(str::to_owned),
            manifest_digest: image.manifest_digest().to_owned(),
            config_digest: image.config_digest().to_owned(),
        })
    }
}

fn sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
