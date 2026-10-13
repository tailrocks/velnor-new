//! Production Linux profile admission is limited to source-pinned Ubuntu 26 images.

use std::time::SystemTime;

use velnor_runner_journal::HostError;

use super::RunnerImageProfile;

const PROFILE_KEY: &str = "ubuntu-26.04-amd64";
const SCALE_SET_NAME: &str = "ubuntu-26.04-scale-set";
const RUNNER_REGISTRY_REPOSITORY: &str = "ghcr.io/actions/actions-runner@";
const DIND_REGISTRY_REPOSITORY: &str = "docker.io/library/docker@";

// No official Ubuntu 26 runner image has passed the registry, OCI, OS, release,
// and host-fact review yet. A future value must be a source-controlled immutable
// record; configuration cannot construct or provide one. Static evidence is
// intentionally separate from the legacy Ubuntu 24 profile kept for old plan
// fixtures and provenance checks.
const ADMITTED_UBUNTU26_PROFILE: Option<RunnerImageProfile> = None;

/// Resolve the exact compiled Ubuntu 26 profile for a production Linux launch.
///
/// No profile is currently compiled because the official image evidence is
/// unavailable. The Ubuntu 24 record is not a fallback. A future profile is
/// accepted only after its immutable source pin passes [`valid_profile_pin`].
///
/// # Errors
///
/// Returns [`HostError::Config`] for the current missing pin, a wrong selector,
/// a malformed source pin, or a stale runner release.
pub fn resolve_linux_admission_profile(
    profile_key: &str,
    scale_set_name: &str,
) -> Result<RunnerImageProfile, HostError> {
    resolve_from_pins(
        profile_key,
        scale_set_name,
        SystemTime::now(),
        ADMITTED_UBUNTU26_PROFILE.as_ref(),
    )
}

/// Validate that a profile is the exact currently compiled Linux admission pin.
///
/// This method does not accept an image identity from configuration or a caller.
/// Profile fields are private and the only production issuer is the immutable
/// source pin table used by [`resolve_linux_admission_profile`].
///
/// # Errors
///
/// Returns [`HostError::Config`] when there is no current Ubuntu 26 pin, the
/// profile does not match that pin, or the pin is stale.
pub fn validate_linux_admission_profile(profile: RunnerImageProfile) -> Result<(), HostError> {
    let approved = ADMITTED_UBUNTU26_PROFILE.ok_or(HostError::Config)?;
    if profile != approved || !valid_profile_pin(&approved) {
        return Err(HostError::Config);
    }
    approved.ensure_fresh(SystemTime::now())
}

fn resolve_from_pins(
    profile_key: &str,
    scale_set_name: &str,
    now: SystemTime,
    approved: Option<&RunnerImageProfile>,
) -> Result<RunnerImageProfile, HostError> {
    if profile_key != PROFILE_KEY || scale_set_name != SCALE_SET_NAME {
        return Err(HostError::Config);
    }
    let profile = approved.copied().ok_or(HostError::Config)?;
    if profile.key != profile_key
        || profile.scale_set_name != scale_set_name
        || !valid_profile_pin(&profile)
    {
        return Err(HostError::Config);
    }
    profile.ensure_fresh(now)?;
    Ok(profile)
}

fn valid_profile_pin(profile: &RunnerImageProfile) -> bool {
    profile.key == PROFILE_KEY
        && profile.scale_set_name == SCALE_SET_NAME
        && profile.platform == "linux/amd64"
        && profile.runner_os == "ubuntu26"
        && image_ref_matches(
            profile.runner_image,
            RUNNER_REGISTRY_REPOSITORY,
            profile.runner_manifest_digest,
        )
        && sha256_digest(profile.runner_manifest_digest)
        && sha256_digest(profile.runner_index_digest)
        && sha256_digest(profile.runner_config_digest)
        && valid_version(profile.runner_version)
        && canonical_utc(profile.runner_release_published_at)
        && canonical_utc(profile.runner_requalify_by)
        && profile.runner_release_published_at < profile.runner_requalify_by
        && image_ref_matches(
            profile.dind_image,
            DIND_REGISTRY_REPOSITORY,
            profile.dind_manifest_digest,
        )
        && sha256_digest(profile.dind_manifest_digest)
        && sha256_digest(profile.dind_index_digest)
        && sha256_digest(profile.dind_config_digest)
        && valid_version(profile.dind_version)
        && has_pinned_dind_source(profile.dind_source)
        && sha256_hex(profile.dind_entrypoint_sha256)
        && profile.runner_uid != 0
        && profile.runner_gid != 0
        && profile.runner_docker_gid != 0
        && profile.dind_socket_group == "docker"
        && profile.dind_socket_gid != 0
}

fn image_ref_matches(reference: &str, repository: &str, manifest: &str) -> bool {
    sha256_digest(manifest) && reference.strip_prefix(repository) == Some(manifest)
}

fn sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(sha256_hex)
}

fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let valid_part = |part: Option<&str>| {
        part.is_some_and(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
    };
    valid_part(parts.next())
        && valid_part(parts.next())
        && valid_part(parts.next())
        && parts.next().is_none()
}

fn canonical_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

fn has_pinned_dind_source(value: &str) -> bool {
    let Some((images_source, dind_source)) = value.split_once(';') else {
        return false;
    };
    pinned_source(images_source, "docker-library/official-images@")
        && dind_source
            .strip_prefix("docker-library/docker@")
            .and_then(|source| source.split_once(':'))
            .is_some_and(|(revision, path)| sha1_hex(revision) && path == "29/dind")
}

fn pinned_source(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(sha1_hex)
}

fn sha1_hex(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
