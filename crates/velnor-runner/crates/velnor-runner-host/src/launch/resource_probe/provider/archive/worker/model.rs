use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use bollard::models::ContainerConfig;

pub(super) const MAX_WORKER_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
pub(super) const MAX_WORKER_ENTRIES: usize = 64;
pub(super) const MAX_METADATA_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_ARCHIVE_METADATA_BYTES: u64 = 2 * 1024 * 1024;
pub(super) const OCI_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub(super) const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub(super) const OCI_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
pub(super) const OCI_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
pub(super) const IN_TOTO_MEDIA_TYPE: &str = "application/vnd.in-toto+json";
pub(super) const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";
pub(super) const BUILD_KIT_TYPE: &str =
    "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md";
pub(super) const SOURCE_REPOSITORY_URL: &str = "https://github.com/tailrocks/velnor-new";
pub(super) const PLATFORM: &str = "linux/amd64";
pub(super) const PATH_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
pub(super) const BASE_LABELS: [&str; 4] = [
    "org.opencontainers.image.created",
    "org.opencontainers.image.description",
    "org.opencontainers.image.title",
    "org.opencontainers.image.version",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::launch::resource_probe::provider) enum WorkerImage {
    Runner,
    Dind,
}

impl WorkerImage {
    pub(in crate::launch::resource_probe::provider) fn archive_name(self) -> &'static str {
        match self {
            Self::Runner => "velnor-runner-linux-amd64.tar",
            Self::Dind => "velnor-dind-linux-amd64.tar",
        }
    }

    pub(in crate::launch::resource_probe::provider) fn image_name(self) -> &'static str {
        match self {
            Self::Runner => "velnor-runner",
            Self::Dind => "velnor-dind",
        }
    }

    pub(super) fn entrypoint(self) -> &'static str {
        match self {
            Self::Runner => "/usr/local/bin/velnor-runner-entrypoint",
            Self::Dind => "/usr/local/bin/velnor-dind-entrypoint",
        }
    }

    pub(super) fn source_context(self) -> &'static str {
        match self {
            Self::Runner => "images/runner/ubuntu-26.04",
            Self::Dind => "images/dind",
        }
    }

    pub(super) fn user(self) -> Option<&'static str> {
        match self {
            Self::Runner => Some("runner"),
            Self::Dind => None,
        }
    }

    pub(super) fn working_dir(self) -> Option<&'static str> {
        match self {
            Self::Runner => Some("/home/runner"),
            Self::Dind => None,
        }
    }

    pub(in crate::launch::resource_probe::provider) fn release_tag(self) -> String {
        format!("{}:linux-amd64", self.image_name())
    }

    pub(super) fn docker_image_name(self) -> String {
        format!("docker.io/library/{}", self.image_name())
    }
}

/// Identities parsed from the archive covered by the release attestation,
/// separate from a daemon's image ID.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::launch::resource_probe::provider) struct ParsedWorkerArchive {
    pub(in crate::launch::resource_probe::provider) archive_sha256: String,
    pub(in crate::launch::resource_probe::provider) archive_index_digest: String,
    pub(in crate::launch::resource_probe::provider) image_index_digest: String,
    pub(in crate::launch::resource_probe::provider) image_index_size: u64,
    pub(in crate::launch::resource_probe::provider) manifest_digest: String,
    pub(in crate::launch::resource_probe::provider) manifest_size: u64,
    pub(in crate::launch::resource_probe::provider) config_digest: String,
    pub(in crate::launch::resource_probe::provider) config: ContainerConfig,
    pub(in crate::launch::resource_probe::provider) diff_ids: Vec<String>,
    pub(in crate::launch::resource_probe::provider) repo_tag: String,
    pub(super) blob_inventory: BTreeMap<String, u64>,
}

impl ParsedWorkerArchive {
    #[cfg(test)]
    pub(super) fn blob_inventory_len(&self) -> usize {
        self.blob_inventory.len()
    }

    pub(super) fn same_image_payload(&self, other: &Self) -> bool {
        self.image_index_digest == other.image_index_digest
            && self.image_index_size == other.image_index_size
            && self.manifest_digest == other.manifest_digest
            && self.manifest_size == other.manifest_size
            && self.config_digest == other.config_digest
            && self.config == other.config
            && self.diff_ids == other.diff_ids
            && self.blob_inventory == other.blob_inventory
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::launch::resource_probe::provider) struct DerivedLoadArchive {
    pub(in crate::launch::resource_probe::provider) path: PathBuf,
    pub(in crate::launch::resource_probe::provider) sha256: String,
    pub(in crate::launch::resource_probe::provider) archive_index_digest: String,
    pub(in crate::launch::resource_probe::provider) repo_tag: String,
    pub(in crate::launch::resource_probe::provider) parsed: ParsedWorkerArchive,
}

pub(super) struct ArchiveFiles {
    pub(super) files: HashMap<String, Vec<u8>>,
    pub(super) blobs: HashMap<String, Blob>,
}

pub(super) struct Blob {
    pub(super) digest: String,
    pub(super) size: u64,
    pub(super) metadata: Option<Vec<u8>>,
}

pub(super) struct VerifiedSelection {
    pub(super) image_index_digest: String,
    pub(super) image_index_size: u64,
    pub(super) manifest_digest: String,
    pub(super) manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
    pub(super) diff_ids: Vec<String>,
    pub(super) repo_tag: String,
    pub(super) blob_inventory: BTreeMap<String, u64>,
}
