// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub(super) const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
pub(super) const PLATFORM: &str = "linux/amd64";
pub(super) const ARCHIVE_FORMAT: &str = "buildkit-oci-layout-docker-compat-v1";
pub(super) const IMAGE_USER: &str = "65532:65532";
pub(super) const IMAGE_ENTRYPOINT: &str = "/velnor/resource-probe";
pub(super) const IMAGE_WORKDIR: &str = "/";
pub(super) const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const DEFAULT_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProbeManifest {
    pub(super) source_commit: String,
    pub(super) workflow_authority_sha: String,
    pub(super) platform: String,
    pub(super) archive_format: String,
    pub(super) oci_index_sha256: String,
    pub(super) image_manifest_digest: String,
    pub(super) config_digest: String,
    pub(super) archive_sha256: String,
}
