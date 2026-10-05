//! Fixed Docker identities for the controller-owned guest probe.

use bollard::models::SystemInfo;

use std::collections::HashMap;

use crate::journal::GuestProbeLease;

use super::{GuestSampleFailure, ProbeContainer, ProbeImage};

/// The code-controlled image tag used by worker containers.
pub(super) const IMAGE_REFERENCE: &str = super::super::super::super::DIND_IMAGE;
const CONTAINER_PREFIX: &str = "velnor-guest-resource-";
const OWNER_LABEL: &str = "velnor.guest-resource.owner";
const ROLE_LABEL: &str = "velnor.guest-resource.role";
const PROBE_ROLE: &str = "guest-resource-probe";

/// Journal owner, selected scale set, and selected daemon identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestProbeIdentity {
    owner_token: String,
    scale_set_id: i64,
    engine_id: String,
}

impl GuestProbeIdentity {
    /// Bind the journal owner to the selected set and Docker engine.
    #[must_use]
    pub(crate) fn from_owner(
        owner: &GuestProbeLease,
        scale_set_id: i64,
        info: &SystemInfo,
    ) -> Result<Self, GuestSampleFailure> {
        let engine_id = info
            .id
            .as_deref()
            .ok_or(GuestSampleFailure::EngineIdentity)?;
        Self::validated(owner.token(), scale_set_id, engine_id)
    }

    fn validated(
        owner_token: &str,
        scale_set_id: i64,
        engine_id: &str,
    ) -> Result<Self, GuestSampleFailure> {
        if !valid_owner_token(owner_token) || scale_set_id <= 0 {
            return Err(GuestSampleFailure::Ownership);
        }
        if !valid_engine_id(engine_id) {
            return Err(GuestSampleFailure::EngineIdentity);
        }
        Ok(Self {
            owner_token: owner_token.to_owned(),
            scale_set_id,
            engine_id: engine_id.to_owned(),
        })
    }

    /// Return the durable journal-scoped owner token.
    #[must_use]
    pub(crate) fn owner_token(&self) -> &str {
        &self.owner_token
    }

    /// Return the Scale Set bound to this sampler identity.
    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Return the selected Docker engine identity bound to the worker.
    #[must_use]
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    #[cfg(test)]
    pub(super) fn test_identity(
        owner_token: &str,
        scale_set_id: i64,
        engine_id: &str,
    ) -> Result<Self, GuestSampleFailure> {
        Self::validated(owner_token, scale_set_id, engine_id)
    }
}

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

/// Stable per-journal/set name used to recover an interrupted probe.
pub(super) fn container_name(identity: &GuestProbeIdentity) -> String {
    format!(
        "{CONTAINER_PREFIX}{}-{}",
        identity.owner_token(),
        identity.scale_set_id()
    )
}

/// Exact labels that authorize probe reconciliation and removal.
pub(super) fn owner_labels(identity: &GuestProbeIdentity) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.product".to_owned(), "velnor".to_owned()),
        ("velnor.guest-resource.set".to_owned(), identity.scale_set_id().to_string()),
        (
            "velnor.guest-resource.engine".to_owned(),
            identity.engine_id().to_owned(),
        ),
        (OWNER_LABEL.to_owned(), identity.owner_token().to_owned()),
        (ROLE_LABEL.to_owned(), PROBE_ROLE.to_owned()),
    ])
}

fn valid_owner_token(value: &str) -> bool {
    value.len() == 32 && lower_hex(value)
}

fn valid_engine_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

/// Confirm a known container ID and the full ownership label set after rename.
pub(super) fn require_owned_container_id(
    container: &ProbeContainer,
    expected_id: &str,
    labels: &HashMap<String, String>,
) -> Result<(), GuestSampleFailure> {
    if container.id.as_deref() != Some(expected_id)
        || &container.labels != labels
        || !valid_container_id(expected_id)
    {
        return Err(GuestSampleFailure::Ownership);
    }
    Ok(())
}

/// Accept only full lowercase Docker container IDs before using an API path.
pub(super) fn valid_container_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{GuestProbeIdentity, container_name, owner_labels};

    #[test]
    fn identity_requires_a_durable_owner_token_set_and_safe_engine_id() {
        assert!(GuestProbeIdentity::test_identity(
            "11111111111111111111111111111111",
            7,
            "engine-a"
        )
        .is_ok());
        assert_eq!(
            GuestProbeIdentity::test_identity(
                "11111111111111111111111111111111",
                7,
                "engine/a"
            ),
            Err(super::GuestSampleFailure::EngineIdentity)
        );
        assert_eq!(
            GuestProbeIdentity::test_identity("not-a-token", 7, "engine-a"),
            Err(super::GuestSampleFailure::Ownership)
        );
        assert_eq!(
            GuestProbeIdentity::test_identity(
                "11111111111111111111111111111111",
                0,
                "engine-a"
            ),
            Err(super::GuestSampleFailure::Ownership)
        );
    }

    #[test]
    fn probe_name_and_labels_bind_journal_set_and_selected_engine() -> Result<(), Box<dyn Error>> {
        let identity = GuestProbeIdentity::test_identity(
            "11111111111111111111111111111111",
            7,
            "engine-a",
        )
        .map_err(|_| std::io::Error::other("valid test identity rejected"))?;
        let name = container_name(&identity);
        let labels = owner_labels(&identity);

        assert!(name.ends_with("11111111111111111111111111111111-7"));
        assert_eq!(
            labels.get("velnor.guest-resource.owner").map(String::as_str),
            Some(identity.owner_token())
        );
        assert_eq!(
            labels
                .get("velnor.guest-resource.engine")
                .map(String::as_str),
            Some(identity.engine_id())
        );
        assert_eq!(
            labels.get("velnor.guest-resource.set").map(String::as_str),
            Some("7")
        );
        Ok(())
    }
}
