//! Create and adopt checks. `disableUpdate` is a registration invariant.

use serde::{Deserialize, Serialize};

use crate::error::WireError;

/// Pascal-case setting object required by the pinned client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerSetting {
    /// Must be true on create and on every re-read.
    #[serde(rename = "disableUpdate")]
    pub disable_update: bool,
}

#[derive(Debug, Serialize)]
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

/// Label object returned by the service.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Label {
    /// Label name.
    pub name: String,
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
    /// Update policy.
    #[serde(rename = "RunnerSetting")]
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
