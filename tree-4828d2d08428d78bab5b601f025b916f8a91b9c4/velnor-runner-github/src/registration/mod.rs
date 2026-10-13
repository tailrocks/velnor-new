//! Create and adopt checks. `disableUpdate` is a registration invariant.
//! HTTP registration calls live beside those checks and inject [`crate::Transport`].

mod admin;
mod discovery;
mod discovery_admin;
mod discovery_async;
mod groups;
mod runners;
mod scale_set;
mod token;

pub use admin::{
    AdminConnection, AdminConnectionCall, admin_connection, admin_connection_once,
    admin_token_is_fresh,
};
pub use discovery::{
    DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId, DiscoveryIntentStore,
    DiscoveryTransport, RepositoryAdminEvidence, RepositoryDiscoveryToken,
    exchange_repository_discovery_admin_once, issue_repository_discovery_token,
    read_repository_admin_evidence,
};
pub use discovery_admin::RepositoryDiscoveryAdmin;
pub use discovery_async::{
    AsyncDiscoveryIntentStore, AsyncDiscoveryTransport, DiscoveryExchange, DiscoveryStoreFuture,
    exchange_repository_discovery_admin_once_async, issue_repository_discovery_token_async,
    read_repository_admin_evidence_async,
};
pub use groups::{RunnerGroup, list_runner_groups};
pub use runners::{get_runner_by_name, remove_runner};
pub use scale_set::{
    ScaleSetById, ScaleSetByName, ScaleSetCreate, ScaleSetFound, create_runner_scale_set,
    get_runner_scale_set, get_runner_scale_set_by_id,
};
pub use token::{
    RegistrationScope, RegistrationToken, RegistrationTokenCall,
    enterprise_registration_token_path, organization_registration_token_path, registration_token,
    repository_registration_token_path,
};

use serde::{Deserialize, Serialize};

use crate::refresh::{RefreshGate, classify_status};
use crate::session::reject;
use crate::{SessionError, WireError};

/// Pascal-case setting object required by the pinned client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerSetting {
    /// Must be true on create and on every re-read.
    #[serde(rename = "disableUpdate")]
    pub disable_update: bool,
}

#[derive(Serialize)]
struct CreateBody<'a> {
    name: &'a str,
    #[serde(rename = "RunnerSetting")]
    runner_setting: RunnerSetting,
}

/// JSON body for create. Always sets `disableUpdate` to true.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty name and
/// [`WireError::Encode`] if serialization fails.
pub fn create_body(name: &str) -> Result<String, WireError> {
    if name.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    let body = CreateBody {
        name,
        runner_setting: RunnerSetting {
            disable_update: true,
        },
    };
    serde_json::to_string(&body).map_err(|_| WireError::Encode)
}

/// One label on a create. An empty `label_type` becomes `System` on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct CreateLabel {
    /// Label name.
    pub name: String,
    /// Service `type`. Empty means `System`.
    pub label_type: String,
}

/// Product labels in wire order. Never the hosted `ubuntu-26.04` label.
pub fn product_create_labels() -> [CreateLabel; 2] {
    [
        CreateLabel {
            name: "velnor".to_owned(),
            label_type: "System".to_owned(),
        },
        CreateLabel {
            name: "ubuntu-26.04-scale-set".to_owned(),
            label_type: "System".to_owned(),
        },
    ]
}

/// Product labels for an explicitly supported Scale Set selector.
///
/// The legacy selector remains available for the existing macOS path. Linux
/// currently uses the official Ubuntu 24.04 runner profile. Image/profile
/// pairing is validated by the host before registration; this client only
/// derives the routing labels from the exact selector.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an unknown selector.
pub fn product_create_labels_for(scale_set_name: &str) -> Result<[CreateLabel; 2], WireError> {
    if !is_supported_product_selector(scale_set_name) {
        return Err(WireError::RegistrationRejected);
    }
    Ok([
        CreateLabel {
            name: "velnor".to_owned(),
            label_type: "System".to_owned(),
        },
        CreateLabel {
            name: scale_set_name.to_owned(),
            label_type: "System".to_owned(),
        },
    ])
}

pub(super) fn is_supported_product_selector(value: &str) -> bool {
    matches!(value, "ubuntu-26.04-scale-set" | "ubuntu-24.04-scale-set")
}

/// Create JSON for the product set: both product labels and `disableUpdate`.
///
/// This legacy helper keeps emitting the Ubuntu 26.04 Scale Set label for
/// existing callers. Use [`http_create_body_for`] for an explicit profile.
/// It is not [`create_body`], which omits labels.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty name and
/// [`WireError::Encode`] if serialization fails.
pub fn http_create_body(name: &str) -> Result<String, WireError> {
    if name.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    outgoing_json(name, &product_create_labels(), 0)
}

/// Create JSON for the product set selected by an explicit supported profile.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an unknown selector and
/// [`WireError::Encode`] if serialization fails.
pub fn http_create_body_for(name: &str) -> Result<String, WireError> {
    let labels = product_create_labels_for(name)?;
    outgoing_json(name, &labels, 0)
}

/// Label object returned by the service.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Label {
    /// Label name.
    pub name: String,
    /// Service `type`. Empty when the payload omits it.
    #[serde(default, rename = "type")]
    pub label_type: String,
}

/// Re-read view. Unknown extra fields are ignored so the service can add them.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ScaleSetView {
    /// Positive scale-set id.
    pub id: i64,
    /// Scale-set name.
    pub name: String,
    /// Returned labels.
    #[serde(default)]
    pub labels: Vec<Label>,
    /// Update policy. Create requests use Pascal-case. The service reads it back camel-case.
    #[serde(rename = "runnerSetting", alias = "RunnerSetting")]
    pub runner_setting: RunnerSetting,
}

/// Refuse a set whose updates are enabled, whose id is not positive, or whose
/// labels include the hosted `ubuntu-26.04` label.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when an invariant fails.
pub fn accept_scale_set(view: &ScaleSetView, expected_name: &str) -> Result<(), WireError> {
    if view.id <= 0 || view.name != expected_name || !view.runner_setting.disable_update {
        return Err(WireError::RegistrationRejected);
    }
    let names: Vec<&str> = view
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();
    if names.contains(&"ubuntu-26.04") {
        return Err(WireError::RegistrationRejected);
    }
    if !names.contains(&"velnor") || !names.contains(&"ubuntu-26.04-scale-set") {
        return Err(WireError::RegistrationRejected);
    }
    Ok(())
}

/// Accept a Scale Set whose label matches one supported product selector.
///
/// The selector is also the expected set name. Linux image-profile pairing is
/// checked by the host before this GitHub API is called.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for unknown selectors, an
/// identity mismatch, enabled runner updates, or missing/conflicting labels.
pub fn accept_scale_set_for(view: &ScaleSetView, expected_name: &str) -> Result<(), WireError> {
    if !is_supported_product_selector(expected_name)
        || view.id <= 0
        || view.name != expected_name
        || !view.runner_setting.disable_update
    {
        return Err(WireError::RegistrationRejected);
    }
    let names: Vec<&str> = view
        .labels
        .iter()
        .map(|label| label.name.as_str())
        .collect();
    if names.contains(&"ubuntu-26.04")
        || !names.contains(&"velnor")
        || !names.contains(&expected_name)
        || names
            .iter()
            .any(|name| is_supported_product_selector(name) && *name != expected_name)
    {
        return Err(WireError::RegistrationRejected);
    }
    Ok(())
}

fn other_status(status: u16) -> SessionError {
    match classify_status(status, &RefreshGate::new()) {
        Ok(class) => reject(class),
        Err(error) => SessionError::from(error),
    }
}

fn outgoing_json(
    name: &str,
    labels: &[CreateLabel],
    runner_group_id: i64,
) -> Result<String, WireError> {
    let prepared = prepare_labels(name, labels)?;
    let body = OutgoingScaleSet {
        name,
        runner_group_id: (runner_group_id > 0).then_some(runner_group_id),
        labels: prepared,
        runner_setting: RunnerSetting {
            disable_update: true,
        },
    };
    serde_json::to_string(&body).map_err(|_| WireError::Encode)
}

fn prepare_labels<'a>(
    name: &'a str,
    labels: &'a [CreateLabel],
) -> Result<Vec<OutgoingLabel<'a>>, WireError> {
    if labels.is_empty() {
        return fallback_label(name);
    }
    let mut prepared = Vec::with_capacity(labels.len());
    for label in labels {
        prepared.push(OutgoingLabel {
            label_type: default_type(label.label_type.as_str()),
            name: label.name.as_str(),
        });
    }
    Ok(prepared)
}

fn fallback_label(name: &str) -> Result<Vec<OutgoingLabel<'_>>, WireError> {
    if name.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(vec![OutgoingLabel {
        label_type: "System",
        name,
    }])
}

const fn default_type(label_type: &str) -> &str {
    if label_type.is_empty() {
        "System"
    } else {
        label_type
    }
}

#[derive(Serialize)]
struct OutgoingScaleSet<'a> {
    name: &'a str,
    #[serde(rename = "runnerGroupId", skip_serializing_if = "Option::is_none")]
    runner_group_id: Option<i64>,
    labels: Vec<OutgoingLabel<'a>>,
    #[serde(rename = "RunnerSetting")]
    runner_setting: RunnerSetting,
}

#[derive(Serialize)]
struct OutgoingLabel<'a> {
    #[serde(rename = "type")]
    label_type: &'a str,
    name: &'a str,
}
