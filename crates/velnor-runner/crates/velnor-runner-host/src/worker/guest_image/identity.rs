// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_runner_image_manifest::ImageKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(super) enum GuestImageIdentity {
    Registry {
        origin: &'static str,
        repository: String,
        release_manifest_sha256: String,
        registry_digest: String,
        manifest_digest: String,
        config_digest: String,
    },
}

pub(super) struct RegistryInput<'a> {
    pub role: ImageKind,
    pub expected_role: ImageKind,
    pub source: &'a str,
    pub expected_source: &'a str,
    pub release_manifest_sha256: &'a str,
    pub expected_release_manifest_sha256: &'a str,
    pub registry_digest: &'a str,
    pub manifest_digest: &'a str,
    pub config_digest: &'a str,
}

pub(super) fn registry(input: &RegistryInput<'_>) -> Option<GuestImageIdentity> {
    if input.role != input.expected_role
        || !lower_hex(input.source, 40)
        || input.source != input.expected_source
        || !lower_hex(input.release_manifest_sha256, 64)
        || input.release_manifest_sha256 != input.expected_release_manifest_sha256
        || !sha256_digest(input.registry_digest)
        || input.registry_digest != input.manifest_digest
        || !sha256_digest(input.config_digest)
    {
        return None;
    }
    Some(GuestImageIdentity::Registry {
        origin: "registry",
        repository: input.role.repository().to_owned(),
        release_manifest_sha256: input.release_manifest_sha256.to_owned(),
        registry_digest: input.registry_digest.to_owned(),
        manifest_digest: input.manifest_digest.to_owned(),
        config_digest: input.config_digest.to_owned(),
    })
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
