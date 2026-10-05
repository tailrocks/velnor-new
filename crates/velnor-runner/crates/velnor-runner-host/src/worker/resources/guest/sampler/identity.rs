//! Fixed Docker identities for the controller-owned guest probe.

use std::collections::HashMap;

use crate::journal::LaunchIdentity;

use super::{GuestSampleFailure, ProbeContainer, ProbeImage};

/// The code-controlled image tag used by worker containers.
pub(super) const IMAGE_REFERENCE: &str = super::super::super::super::DIND_IMAGE;
const CONTAINER_PREFIX: &str = "velnor-guest-resource-";
const OWNER_LABEL: &str = "velnor.guest-resource.owner";
const ROLE_LABEL: &str = "velnor.guest-resource.role";
const PROBE_ROLE: &str = "guest-resource-probe";

/// Require an exact local tag and immutable Linux AMD64 image identity.
pub(super) fn trusted_image_id(image: &ProbeImage) -> Result<String, GuestSampleFailure> {
    let id = image.id.as_deref().ok_or(GuestSampleFailure::ProbeImage)?;
    let digest = id
        .strip_prefix("sha256:")
        .ok_or(GuestSampleFailure::ProbeImage)?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !image.repo_tags.iter().any(|tag| tag == IMAGE_REFERENCE)
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
    {
        return Err(GuestSampleFailure::ProbeImage);
    }
    Ok(id.to_owned())
}

/// Stable per-instance name used to recover an interrupted probe.
pub(super) fn container_name(identity: &LaunchIdentity) -> String {
    format!("{CONTAINER_PREFIX}{}", identity.instance_id())
}

/// Exact labels that authorize probe reconciliation and removal.
pub(super) fn owner_labels(identity: &LaunchIdentity) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.product".to_owned(), "velnor".to_owned()),
        (
            "velnor.instance".to_owned(),
            identity.instance_id().to_owned(),
        ),
        ("velnor.engine".to_owned(), identity.engine_id().to_owned()),
        (
            OWNER_LABEL.to_owned(),
            format!("{}:{}", identity.engine_id(), identity.instance_id()),
        ),
        (ROLE_LABEL.to_owned(), PROBE_ROLE.to_owned()),
    ])
}

/// Return an ID only when name, labels, and Docker ID all match.
pub(super) fn require_owned_id(
    container: &ProbeContainer,
    name: &str,
    labels: &HashMap<String, String>,
) -> Result<String, GuestSampleFailure> {
    let found_name = match container.name.as_deref() {
        Some(found) => match found.strip_prefix('/') {
            Some(stripped) => stripped,
            None => found,
        },
        None => return Err(GuestSampleFailure::Ownership),
    };
    let id = container
        .id
        .as_deref()
        .ok_or(GuestSampleFailure::Ownership)?;
    if found_name != name || &container.labels != labels || !valid_container_id(id) {
        return Err(GuestSampleFailure::Ownership);
    }
    Ok(id.to_owned())
}

/// Accept only full lowercase Docker container IDs before using an API path.
pub(super) fn valid_container_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
