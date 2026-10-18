//! Store-specific image inspection evidence recorded on completion.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::identity::{ImageImportRequest, valid_digest};

const OCI_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// Containerd descriptor fields observed on the selected daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageImportDescriptor {
    pub(crate) digest: String,
    pub(crate) media_type: String,
    pub(crate) size: u64,
    pub(crate) annotations: BTreeMap<String, String>,
    pub(crate) has_extra_fields: bool,
}

/// Image identity returned by the selected daemon's image store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ImageImportObservation {
    /// Classic Docker image store, which identifies an image by config digest.
    Classic {
        image_id: String,
        repo_tags: Vec<String>,
    },
    /// Containerd image store, which identifies the selected image descriptor.
    Containerd {
        image_id: String,
        repo_tags: Vec<String>,
        repo_digests: Vec<String>,
        descriptor: ImageImportDescriptor,
    },
}

impl ImageImportObservation {
    pub(crate) fn validate_for(&self, request: &ImageImportRequest) -> Result<(), HostError> {
        request.validate()?;
        match self {
            Self::Classic {
                image_id,
                repo_tags,
            } => {
                if image_id != &request.config_digest
                    || repo_tags.len() != 1
                    || repo_tags[0] != request.image_reference
                {
                    return Err(HostError::Identity);
                }
            }
            Self::Containerd {
                image_id,
                repo_tags,
                repo_digests,
                descriptor,
            } => {
                validate_containerd(request, image_id, repo_tags, repo_digests, descriptor)?;
            }
        }
        Ok(())
    }

    pub(crate) fn image_id(&self) -> &str {
        match self {
            Self::Classic { image_id, .. } | Self::Containerd { image_id, .. } => image_id,
        }
    }

    pub(crate) fn from_receipt(value: &str) -> Result<Self, HostError> {
        let observation: Self = serde_json::from_str(value).map_err(|_| HostError::Journal)?;
        if serde_json::to_string(&observation).map_err(|_| HostError::Journal)? != value {
            return Err(HostError::Journal);
        }
        Ok(observation)
    }

    pub(super) fn receipt(&self) -> Result<String, HostError> {
        serde_json::to_string(self).map_err(|_| HostError::Journal)
    }
}

fn validate_containerd(
    request: &ImageImportRequest,
    image_id: &str,
    repo_tags: &[String],
    repo_digests: &[String],
    descriptor: &ImageImportDescriptor,
) -> Result<(), HostError> {
    let (name, tag) = request
        .image_reference
        .split_once(':')
        .ok_or(HostError::Journal)?;
    let digest = request.image_id_digest();
    let media_type = if request.image_index_digest.is_some() {
        OCI_INDEX_MEDIA_TYPE
    } else {
        OCI_MANIFEST_MEDIA_TYPE
    };
    let expected_annotations = BTreeMap::from([
        (
            "io.containerd.image.name".to_owned(),
            format!("docker.io/library/{}:{}", name, tag),
        ),
        (
            "org.opencontainers.image.ref.name".to_owned(),
            tag.to_owned(),
        ),
    ]);
    let expected_repo_digest = format!("docker.io/library/{name}@{digest}");
    let repo_digests_match = repo_digests.is_empty()
        || (repo_digests.len() == 1 && repo_digests[0] == expected_repo_digest);
    if image_id != digest
        || !valid_digest(image_id)
        || repo_tags.len() != 1
        || repo_tags[0] != request.image_reference
        || !repo_digests_match
        || descriptor.digest != digest
        || descriptor.media_type != media_type
        || descriptor.size != request.image_descriptor_size()
        || descriptor.annotations != expected_annotations
        || descriptor.has_extra_fields
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
