use bollard::models::{Mount, MountPoint, MountType};
use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::{BootstrapStartClaim, VerifiedMetadataInitializationHandoff};
use crate::launch::bootstrap::VerifiedBootstrapMountHandoff;

#[cfg(test)]
mod tests;

pub(super) fn inspect_volume_mounts(
    inspected: &bollard::models::ContainerInspectResponse,
    expected: &[Mount],
    role: &str,
) -> Result<String, HostError> {
    let configured = inspected
        .host_config
        .as_ref()
        .and_then(|host| host.mounts.as_deref())
        .ok_or(HostError::Ownership)?;
    if configured != expected {
        return Err(HostError::Ownership);
    }
    let actual = inspected.mounts.as_deref().ok_or(HostError::Ownership)?;
    let expected_tmpfs = inspected
        .host_config
        .as_ref()
        .and_then(|host| host.tmpfs.as_ref())
        .ok_or(HostError::Ownership)?;
    let expected_tmpfs_targets = verified_tmpfs_targets(expected_tmpfs, actual)?;
    let (expected_volumes, actual_volumes) = verified_volume_mounts(expected, actual)?;
    let mut bytes = Vec::new();
    append(&mut bytes, role)?;
    for (name, target, subpath, read_only) in expected_volumes {
        append(&mut bytes, &name)?;
        append(&mut bytes, &target)?;
        append(&mut bytes, &subpath)?;
        append(&mut bytes, if read_only { "ro" } else { "rw" })?;
    }
    for (name, target, read_only) in actual_volumes {
        append(&mut bytes, &name)?;
        append(&mut bytes, &target)?;
        append(&mut bytes, if read_only { "ro" } else { "rw" })?;
    }
    for (target, options) in expected_tmpfs_targets {
        append(&mut bytes, target)?;
        append(&mut bytes, options)?;
    }
    Ok(lower_hex(&Sha256::digest(bytes)))
}

fn verified_tmpfs_targets<'a>(
    expected: &'a std::collections::HashMap<String, String>,
    actual: &[MountPoint],
) -> Result<Vec<(&'a String, &'a String)>, HostError> {
    let mut expected_targets = expected.iter().collect::<Vec<_>>();
    let mut actual_targets = actual
        .iter()
        .filter(|mount| mount.typ.as_deref() == Some("tmpfs"))
        .map(|mount| {
            if mount.rw != Some(true) {
                return Err(HostError::Ownership);
            }
            mount.destination.clone().ok_or(HostError::Ownership)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if actual
        .iter()
        .any(|mount| !matches!(mount.typ.as_deref(), Some("volume" | "tmpfs")))
    {
        return Err(HostError::Ownership);
    }
    expected_targets.sort_by(|left, right| left.0.cmp(right.0));
    actual_targets.sort();
    if expected_targets
        .iter()
        .map(|(target, _)| target.as_str())
        .collect::<Vec<_>>()
        != actual_targets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        return Err(HostError::Ownership);
    }
    Ok(expected_targets)
}

fn verified_volume_mounts(
    expected: &[Mount],
    actual: &[MountPoint],
) -> Result<(Vec<ExpectedVolume>, Vec<ActualVolume>), HostError> {
    let expected_volumes = expected
        .iter()
        .map(expected_volume)
        .collect::<Result<Vec<_>, _>>()?;
    let mut actual_volumes = actual
        .iter()
        .filter(|mount| mount.typ.as_deref() == Some("volume"))
        .map(actual_volume)
        .collect::<Result<Vec<_>, _>>()?;
    if actual_volumes.len() != expected_volumes.len() {
        return Err(HostError::Ownership);
    }
    let mut expected_effective = expected_volumes
        .iter()
        .map(|(name, target, _, read_only)| (name.clone(), target.clone(), *read_only))
        .collect::<Vec<_>>();
    actual_volumes.sort();
    expected_effective.sort();
    if expected_effective != actual_volumes
        || has_duplicate_targets(&expected_effective)
        || has_duplicate_actual_targets(&actual_volumes)
    {
        return Err(HostError::Ownership);
    }
    Ok((expected_volumes, actual_volumes))
}

pub(super) fn create_handoff(
    metadata: &VerifiedMetadataInitializationHandoff,
    claim: &BootstrapStartClaim,
    runner_digest: &str,
    dind_digest: &str,
) -> Result<VerifiedBootstrapMountHandoff, HostError> {
    if metadata.reservation_id() != claim.reservation_id()
        || metadata.operation_id() != claim.operation_id()
        || metadata.engine_id() != claim.engine_id()
        || metadata.docker_root_digest() != claim.docker_root_digest()
        || !valid_digest(runner_digest)
        || !valid_digest(dind_digest)
    {
        return Err(HostError::Identity);
    }
    let mut bytes = Vec::with_capacity(128);
    append(&mut bytes, runner_digest)?;
    append(&mut bytes, dind_digest)?;
    Ok(VerifiedBootstrapMountHandoff {
        connection_id: metadata.connection_id().to_owned(),
        reservation_id: metadata.reservation_id(),
        operation_id: metadata.operation_id(),
        engine_id: metadata.engine_id().to_owned(),
        docker_root_digest: metadata.docker_root_digest().to_owned(),
        metadata_volume_name: metadata.metadata_volume_name().to_owned(),
        metadata_volume_id: metadata.metadata_volume_id().to_owned(),
        layout_digest: metadata.layout_digest().to_owned(),
        runner_container_id: claim.runner_container_id().to_owned(),
        dind_container_id: claim.dind_container_id().to_owned(),
        mount_evidence_digest: lower_hex(&Sha256::digest(bytes)),
    })
}

type ExpectedVolume = (String, String, String, bool);
type ActualVolume = (String, String, bool);

fn expected_volume(mount: &Mount) -> Result<ExpectedVolume, HostError> {
    if mount.typ != Some(MountType::VOLUME) {
        return Err(HostError::Identity);
    }
    let name = mount.source.clone().ok_or(HostError::Identity)?;
    let target = mount.target.clone().ok_or(HostError::Identity)?;
    let read_only = mount.read_only.ok_or(HostError::Identity)?;
    let options = mount.volume_options.as_ref().ok_or(HostError::Identity)?;
    if options.no_copy != Some(true) {
        return Err(HostError::Identity);
    }
    Ok((
        name,
        target,
        options.subpath.clone().unwrap_or_default(),
        read_only,
    ))
}

fn actual_volume(mount: &MountPoint) -> Result<ActualVolume, HostError> {
    if mount.typ.as_deref() != Some("volume") {
        return Err(HostError::Identity);
    }
    Ok((
        mount.name.clone().ok_or(HostError::Identity)?,
        mount.destination.clone().ok_or(HostError::Identity)?,
        !mount.rw.ok_or(HostError::Identity)?,
    ))
}

fn append(target: &mut Vec<u8>, value: &str) -> Result<(), HostError> {
    let length = u32::try_from(value.len()).map_err(|_| HostError::Identity)?;
    target.extend_from_slice(&length.to_be_bytes());
    target.extend_from_slice(value.as_bytes());
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn has_duplicate_targets(values: &[(String, String, bool)]) -> bool {
    values
        .iter()
        .map(|value| value.1.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != values.len()
}

fn has_duplicate_actual_targets(values: &[(String, String, bool)]) -> bool {
    has_duplicate_targets(values)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
