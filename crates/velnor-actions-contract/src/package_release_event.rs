//! Closed native-preview admission, separate from generic workflow triggers.

use super::PackageReleaseChannel;
use serde::{Deserialize, Serialize};

/// The only event authorized to produce an automatic native preview.
/// Generic workflow trigger validation continues to reject this privileged event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePreviewEvent {
    /// Completion of independently authenticated default-branch push CI.
    WorkflowRun,
}

impl NativePreviewEvent {
    /// Actual GitHub event name, never the upstream CI's push event.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowRun => "workflow_run",
        }
    }
}

/// Actual original producing/signing workflow event carried by release evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageReleaseEvent {
    /// Native preview triggered by completed CI.
    WorkflowRun,
    /// Stable release explicitly dispatched from the configured default branch.
    WorkflowDispatch,
}

impl PackageReleaseEvent {
    /// Exact event authenticated in certificate and SLSA claims.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowRun => NativePreviewEvent::WorkflowRun.as_str(),
            Self::WorkflowDispatch => "workflow_dispatch",
        }
    }
}

impl PackageReleaseChannel {
    /// Canonical generated release workflow display name.
    #[must_use]
    pub const fn workflow_name(self) -> &'static str {
        match self {
            Self::Preview => "Rust binary preview",
            Self::Stable => "Rust binary release",
        }
    }

    /// Fixed default-branch continuation workflow filename.
    #[must_use]
    pub const fn tap_reconcile_workflow_path(self) -> &'static str {
        match self {
            Self::Preview => ".github/workflows/preview-tap-reconcile.yml",
            Self::Stable => ".github/workflows/stable-tap-reconcile.yml",
        }
    }

    /// Canonical display name authenticated for the retained-tap continuation.
    #[must_use]
    pub const fn tap_reconcile_workflow_name(self) -> &'static str {
        match self {
            Self::Preview => "Preview tap reconciliation",
            Self::Stable => "Stable tap reconciliation",
        }
    }

    /// Original producing event required by this release channel.
    #[must_use]
    pub const fn event(self) -> PackageReleaseEvent {
        match self {
            Self::Preview => PackageReleaseEvent::WorkflowRun,
            Self::Stable => PackageReleaseEvent::WorkflowDispatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_preview_event_does_not_relax_generic_trigger_admission() {
        assert_eq!(NativePreviewEvent::WorkflowRun.as_str(), "workflow_run");
        assert!(crate::validate_workflow_trigger_event("workflow_run").is_err());
        for event in ["push", "workflow_dispatch", "pull_request"] {
            assert!(
                serde_json::from_value::<NativePreviewEvent>(serde_json::json!(event)).is_err()
            );
        }
        assert!(serde_json::from_value::<PackageReleaseEvent>(serde_json::json!("push")).is_err());
    }
}
