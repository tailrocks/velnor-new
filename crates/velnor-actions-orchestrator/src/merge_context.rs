use std::path::{Path, PathBuf};

/// Trusted in-process context for a final merge evaluation.
///
/// This type is not deserializable. Request JSON cannot claim a repository
/// workspace or opt out of the native-plan workspace requirement.
#[derive(Clone, Debug)]
pub struct MergeContext {
    repository_present: bool,
    workspace: Option<PathBuf>,
}

impl MergeContext {
    /// Construct a detached context for synthetic non-native merge requests.
    #[must_use]
    pub fn detached() -> Self {
        Self {
            repository_present: false,
            workspace: None,
        }
    }

    /// Construct a context that requires native checks to validate a workspace.
    ///
    /// `None` represents a runner that identifies itself as a repository
    /// workflow but has no workspace available; merge then fails closed.
    #[must_use]
    pub fn required_workspace(workspace: Option<PathBuf>) -> Self {
        Self {
            repository_present: true,
            workspace,
        }
    }

    pub(super) fn capture_ambient() -> Self {
        Self {
            repository_present: std::env::var_os("GITHUB_REPOSITORY").is_some(),
            workspace: std::env::var_os("GITHUB_WORKSPACE").map(PathBuf::from),
        }
    }

    pub(super) const fn repository_present(&self) -> bool {
        self.repository_present
    }

    pub(super) fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }
}
