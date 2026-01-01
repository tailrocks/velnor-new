//! Resolve the action-owned MBX from the same filtered environment that runs it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{EnvPolicy, IsolatedCommand};
use crate::MiseError;

/// Absolute action-owned executable resolved from one effective child PATH.
///
/// The path is deliberately not canonicalized: an action-provided symlink or
/// wrapper keeps its invocation path, while callers cannot construct this
/// capability from an arbitrary string.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ActionOwnedMbxPath(PathBuf);

impl ActionOwnedMbxPath {
    pub(crate) fn as_os_str(&self) -> &OsStr {
        self.0.as_os_str()
    }
}

impl IsolatedCommand {
    /// Resolve the first executable `mbx` in this command's effective child PATH.
    ///
    /// The command must use the MBX policy. Resolving from the policy-filtered
    /// environment keeps Mise wrappers out of selection while retaining the
    /// action-added MBX directory. The returned absolute path can then be
    /// reused across a version probe and its guarded payload.
    ///
    /// # Errors
    /// Fails closed if PATH has no absolute executable MBX entry.
    pub(crate) fn resolve_action_owned_mbx(&self) -> Result<ActionOwnedMbxPath, MiseError> {
        if self.policy != EnvPolicy::Mbx {
            return Err(MiseError::InvalidStepInput {
                field: "action_owned_mbx".to_owned(),
                value: "requires_mbx_environment_policy".to_owned(),
            });
        }
        let parent: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let effective = self.spawn_env(&parent);
        resolve_action_owned_mbx_in(&effective)
    }
}

fn resolve_action_owned_mbx_in(
    environment: &[(OsString, OsString)],
) -> Result<ActionOwnedMbxPath, MiseError> {
    let path = environment
        .iter()
        .rev()
        .find(|(key, _)| key == OsStr::new("PATH"))
        .map(|(_, value)| value)
        .ok_or_else(mbx_not_found)?;
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("mbx");
        if !is_executable_file(&candidate) {
            continue;
        }
        return Ok(ActionOwnedMbxPath(candidate));
    }
    Err(mbx_not_found())
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn mbx_not_found() -> MiseError {
    MiseError::SpawnFailed {
        program: "mbx".to_owned(),
        message: "action_owned_binary_not_found_on_path".to_owned(),
    }
}

#[cfg(all(test, unix))]
#[path = "command_action_mbx_tests.rs"]
mod tests;
