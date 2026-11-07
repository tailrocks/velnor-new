// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

pub const PROTECTED_BASE_CONTROLLER_PATH: &str = "$RUNNER_TEMP/velnor/bin/controller/binary";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerLocator {
    PinnedRelease {
        version: String,
        path: String,
    },
    ProtectedBaseDispatch {
        preview: Box<velnor_actions_contract::source_preview::SourcePreviewLock>,
        path: String,
    },
}

impl ControllerLocator {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::PinnedRelease { path, .. } | Self::ProtectedBaseDispatch { path, .. } => path,
        }
    }

    #[must_use]
    pub fn pinned_version(&self) -> Option<&str> {
        match self {
            Self::PinnedRelease { version, .. } => Some(version),
            Self::ProtectedBaseDispatch { .. } => None,
        }
    }

    #[must_use]
    pub fn report_helper_version(&self) -> &str {
        match self {
            Self::PinnedRelease { version, .. } => version,
            Self::ProtectedBaseDispatch { preview, .. } => &preview.receipt.version,
        }
    }

    pub fn require_pinned_version(&self) -> Result<&str, RenderError> {
        self.pinned_version().ok_or_else(|| {
            RenderError::InvalidWorkflow("preview_runtime_identity_unavailable".to_owned())
        })
    }

    pub fn validate(&self) -> Result<(), RenderError> {
        match self {
            Self::PinnedRelease { version, path } => {
                let parsed = semver::Version::parse(version)
                    .map_err(|_| RenderError::BadVersion(version.clone()))?;
                if !parsed.pre.is_empty() || !parsed.build.is_empty() {
                    return Err(RenderError::BadVersion(version.clone()));
                }
                crate::guard::validate_staged_binary(path, version)
            }
            Self::ProtectedBaseDispatch { preview, path } => {
                preview.validate_shape().map_err(RenderError::Contract)?;
                if path != PROTECTED_BASE_CONTROLLER_PATH {
                    return Err(RenderError::InvalidWorkflow(
                        "untrusted_controller_dispatch_path".to_owned(),
                    ));
                }
                Ok(())
            }
        }
    }
}
