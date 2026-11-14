// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
pub(super) const MAX_WORKER_ENTRIES: usize = 64;
pub(super) const MAX_METADATA_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_ARCHIVE_METADATA_BYTES: u64 = 2 * 1024 * 1024;
pub(super) const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub(super) const OCI_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
pub(super) const IN_TOTO_MEDIA_TYPE: &str = "application/vnd.in-toto+json";
pub(super) const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";
pub(super) const BUILD_KIT_TYPE: &str =
    "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md";
pub(super) const SOURCE_REPOSITORY_URL: &str = "https://github.com/tailrocks/velnor-new";
pub(super) const PLATFORM: &str = "linux/amd64";
pub(super) const PATH_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
pub(super) const BASE_LABELS: [&str; 6] = [
    "org.opencontainers.image.created",
    "org.opencontainers.image.description",
    "org.opencontainers.image.title",
    "org.opencontainers.image.version",
    "io.velnor.image.kind",
    "org.opencontainers.image.revision",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerImage {
    Runner,
    Dind,
}

impl WorkerImage {
    pub(super) fn image_kind(self) -> &'static str {
        match self {
            Self::Runner => "runner",
            Self::Dind => "dind",
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

    pub(super) fn allowed_local_names(self) -> &'static [&'static str] {
        match self {
            Self::Runner => &[
                "context",
                "dockerfile",
                "resource_probe",
                "storage_init",
                "job_start_hook",
                "job_started_hook",
            ],
            Self::Dind => &["context", "dockerfile", "resource_probe"],
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
}

pub(super) struct ArchiveFiles {
    pub(super) files: HashMap<String, Vec<u8>>,
    pub(super) blobs: HashMap<String, Blob>,
}

pub(super) struct Blob {
    pub(super) offset: Option<u64>,
    pub(super) digest: String,
    pub(super) size: u64,
    pub(super) metadata: Option<Vec<u8>>,
}
