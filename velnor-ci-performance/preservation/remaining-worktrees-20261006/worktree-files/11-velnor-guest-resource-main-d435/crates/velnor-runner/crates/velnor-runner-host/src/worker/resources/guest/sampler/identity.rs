//! Fixed Docker identities for the controller-owned guest probe.

use std::collections::HashMap;

use bollard::models::SystemInfo;

use crate::reconcile::IntentRow;

use super::{GuestSampleFailure, ProbeContainer, ProbeImage};

/// The code-controlled image tag used by worker containers.
pub(super) const IMAGE_REFERENCE: &str = super::super::super::super::DIND_IMAGE;
const CONTAINER_PREFIX: &str = "velnor-guest-resource-";
const OWNER_LABEL: &str = "velnor.guest-resource.owner";
const ROLE_LABEL: &str = "velnor.guest-resource.role";
const PROBE_ROLE: &str = "guest-resource-probe";

/// Durable worker volume paired with the selected daemon identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestProbeIdentity {
    worker_volume: String,
    engine_id: String,
}

impl GuestProbeIdentity {
    /// Bind a journal row to the ID reported by its selected Docker client.
    #[must_use]
    pub(crate) fn from_selected_worker(
        row: &IntentRow,
        info: &SystemInfo,
    ) -> Result<Self, GuestSampleFailure> {
        let worker_volume = row
            .worker_volume
            .as_deref()
            .ok_or(GuestSampleFailure::Ownership)?;
        let engine_id = info
            .id
            .as_deref()
            .ok_or(GuestSampleFailure::EngineIdentity)?;
        Self::validated(worker_volume, engine_id)
    }

    fn validated(worker_volume: &str, engine_id: &str) -> Result<Self, GuestSampleFailure> {
        if !valid_worker_volume(worker_volume) {
            return Err(GuestSampleFailure::Ownership);
        }
        if !valid_engine_id(engine_id) {
            return Err(GuestSampleFailure::EngineIdentity);
        }
        Ok(Self {
            worker_volume: worker_volume.to_owned(),
            engine_id: engine_id.to_owned(),
        })
    }

    /// Return the durable volume identity used by this worker.
    #[must_use]
    pub(crate) fn worker_volume(&self) -> &str {
        &self.worker_volume
    }

    /// Return the selected Docker engine identity bound to the worker.
    #[must_use]
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
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

/// Stable per-worker name used to recover an interrupted probe.
pub(super) fn container_name(identity: &GuestProbeIdentity) -> String {
    format!("{CONTAINER_PREFIX}{}", identity.worker_volume())
}

/// Exact labels that authorize probe reconciliation and removal.
pub(super) fn owner_labels(identity: &GuestProbeIdentity) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.product".to_owned(), "velnor".to_owned()),
        (
            "velnor.worker-volume".to_owned(),
            identity.worker_volume().to_owned(),
        ),
        ("velnor.engine".to_owned(), identity.engine_id().to_owned()),
        (
            OWNER_LABEL.to_owned(),
            format!("{}:{}", identity.engine_id(), identity.worker_volume()),
        ),
        (ROLE_LABEL.to_owned(), PROBE_ROLE.to_owned()),
    ])
}

fn valid_worker_volume(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix('w') else {
        return false;
    };
    suffix.len() == 32 && lower_hex(suffix)
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

    use bollard::models::SystemInfo;

    use crate::journal::IntentState;
    use crate::reconcile::IntentRow;

    use super::{GuestProbeIdentity, container_name, owner_labels};

    #[test]
    fn identity_requires_a_durable_worker_volume_and_safe_engine_id() {
        let row = intent_row(Some("w11111111111111111111111111111111"));
        let info = engine_info(Some("engine-a"));
        assert!(GuestProbeIdentity::from_selected_worker(&row, &info).is_ok());
        let info = engine_info(Some("engine/a"));
        assert_eq!(
            GuestProbeIdentity::from_selected_worker(&row, &info),
            Err(super::GuestSampleFailure::EngineIdentity)
        );
        let row = intent_row(Some("v11111111111111111111111111111111"));
        let info = engine_info(Some("engine-a"));
        assert_eq!(
            GuestProbeIdentity::from_selected_worker(&row, &info),
            Err(super::GuestSampleFailure::Ownership)
        );
        let row = intent_row(None);
        assert_eq!(
            GuestProbeIdentity::from_selected_worker(&row, &info),
            Err(super::GuestSampleFailure::Ownership)
        );
        let row = intent_row(Some("w11111111111111111111111111111111"));
        let info = engine_info(None);
        assert_eq!(
            GuestProbeIdentity::from_selected_worker(&row, &info),
            Err(super::GuestSampleFailure::EngineIdentity)
        );
    }

    #[test]
    fn probe_name_and_labels_bind_worker_volume_to_selected_engine() -> Result<(), Box<dyn Error>> {
        let row = intent_row(Some("w11111111111111111111111111111111"));
        let info = engine_info(Some("engine-a"));
        let identity = GuestProbeIdentity::from_selected_worker(&row, &info)
            .map_err(|_| std::io::Error::other("valid test identity rejected"))?;
        let name = container_name(&identity);
        let labels = owner_labels(&identity);

        assert!(name.ends_with(identity.worker_volume()));
        assert_eq!(
            labels.get("velnor.worker-volume").map(String::as_str),
            Some(identity.worker_volume())
        );
        assert_eq!(
            labels.get("velnor.engine").map(String::as_str),
            Some(identity.engine_id())
        );
        assert_eq!(
            labels
                .get("velnor.guest-resource.owner")
                .map(String::as_str),
            Some("engine-a:w11111111111111111111111111111111")
        );
        Ok(())
    }

    fn intent_row(worker_volume: Option<&str>) -> IntentRow {
        IntentRow {
            id: 1,
            kind: "acquire".to_owned(),
            subject: "test".to_owned(),
            state: IntentState::Pending,
            docker_id: None,
            dind_id: None,
            worker_volume: worker_volume.map(str::to_owned),
            github_runner_id: None,
            cleanup_proven: false,
        }
    }

    fn engine_info(id: Option<&str>) -> SystemInfo {
        SystemInfo {
            id: id.map(str::to_owned),
            ..Default::default()
        }
    }
}
