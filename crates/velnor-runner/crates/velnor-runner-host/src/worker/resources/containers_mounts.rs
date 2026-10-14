use std::collections::HashSet;

use super::super::super::CreateProjection;
use super::super::super::mounts::mount_source;
use crate::error::HostError;

pub(super) fn expected_mounts(
    spec: &CreateProjection,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let mut mounts = HashSet::with_capacity(spec.mounts.len() + spec.bind_mounts.len());
    for mount in &spec.mounts {
        let (_, source) = mount_source(&mount.source)?;
        mounts.insert(("volume".to_owned(), source, mount.target.clone(), true));
    }
    for mount in &spec.bind_mounts {
        mounts.insert((
            "bind".to_owned(),
            mount.source.clone(),
            mount.target.clone(),
            !mount.read_only,
        ));
    }
    Ok(mounts)
}

pub(super) fn observed_mounts(
    found: &bollard::models::ContainerInspectResponse,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let records = found.mounts.as_ref().ok_or(HostError::Ownership)?;
    let mut mounts = HashSet::with_capacity(records.len());
    for mount in records {
        let typ = mount.typ.as_ref().ok_or(HostError::Ownership)?;
        let source = match typ.as_str() {
            "volume" => mount.name.as_ref(),
            "bind" => mount.source.as_ref(),
            _ => return Err(HostError::Ownership),
        }
        .ok_or(HostError::Ownership)?;
        mounts.insert((
            typ.clone(),
            source.clone(),
            mount.destination.clone().ok_or(HostError::Ownership)?,
            mount.rw.ok_or(HostError::Ownership)?,
        ));
    }
    Ok(mounts)
}
