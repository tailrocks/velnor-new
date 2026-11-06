// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub(super) const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
pub(super) const IMAGE_USER: &str = "65532:65532";
pub(super) const IMAGE_ENTRYPOINT: &str = "/velnor/resource-probe";
pub(super) const IMAGE_WORKDIR: &str = "/";
pub(super) const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const DEFAULT_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
