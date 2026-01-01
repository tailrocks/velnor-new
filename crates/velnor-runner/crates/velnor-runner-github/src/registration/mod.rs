//! Create and adopt checks. `disableUpdate` is a registration invariant.
//! HTTP registration calls live beside those checks and inject [`crate::Transport`].

mod admin;
mod groups;
mod scale_set;
mod token;

pub use admin::{AdminConnection, AdminConnectionCall, admin_connection, admin_token_is_fresh};
pub use groups::{RunnerGroup, list_runner_groups};
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

/// Create JSON for the product set: both product labels and `disableUpdate`.
///
/// This is not [`create_body`], which omits labels.
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
