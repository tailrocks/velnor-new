use super::PackageReleaseChannel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePreviewEvent {
    WorkflowRun,
}

impl NativePreviewEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowRun => "workflow_run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageReleaseEvent {
    WorkflowRun,

    WorkflowDispatch,
}

impl PackageReleaseEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowRun => NativePreviewEvent::WorkflowRun.as_str(),
            Self::WorkflowDispatch => "workflow_dispatch",
        }
    }
}

impl PackageReleaseChannel {
    #[must_use]
    pub const fn workflow_name(self) -> &'static str {
        match self {
            Self::Preview => "Rust binary preview",
            Self::Stable => "Rust binary release",
        }
    }

    #[must_use]
    pub const fn tap_reconcile_workflow_path(self) -> &'static str {
        match self {
            Self::Preview => ".github/workflows/preview-tap-reconcile.yml",
            Self::Stable => ".github/workflows/stable-tap-reconcile.yml",
        }
    }

    #[must_use]
    pub const fn tap_reconcile_workflow_name(self) -> &'static str {
        match self {
            Self::Preview => "Preview tap reconciliation",
            Self::Stable => "Stable tap reconciliation",
        }
    }

    #[must_use]
    pub const fn event(self) -> PackageReleaseEvent {
        match self {
            Self::Preview => PackageReleaseEvent::WorkflowRun,
            Self::Stable => PackageReleaseEvent::WorkflowDispatch,
        }
    }
}
