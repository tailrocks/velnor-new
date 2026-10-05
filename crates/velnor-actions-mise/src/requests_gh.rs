//! Fixed GitHub API request path with user config disabled.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;

use super::PinnedToolExec;

impl PinnedToolExec {
    /// Build a fixed GitHub API request with a per-call empty CLI config.
    ///
    /// The caller keeps `config_dir` alive for the command and supplies a
    /// fresh empty directory. Normal gh requests cannot set `GH_CONFIG_DIR`;
    /// this typed path accepts only pinned `gh api repos/... --hostname
    /// github.com --jq .default_branch|.protected` requests, preventing user
    /// config from selecting another API host.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] unless the command is an exact
    /// pinned-gh API request for the official host and `config_dir` is a fresh,
    /// empty absolute directory.
    pub fn command_with_isolated_gh_config(
        &self,
        catalog: &ToolCatalog,
        config_dir: &Path,
    ) -> Result<IsolatedCommand, MiseError> {
        if !is_official_repository_api_query(&self.tools, &self.program, &self.args) {
            return Err(MiseError::InvalidStepInput {
                field: "gh_api_request".to_owned(),
                value: "requires_pinned_official_repository_query".to_owned(),
            });
        }
        if !is_fresh_empty_directory(config_dir) {
            return Err(MiseError::InvalidStepInput {
                field: "GH_CONFIG_DIR".to_owned(),
                value: "requires_fresh_empty_absolute_directory".to_owned(),
            });
        }
        self.command(catalog)?
            .with_internal_gh_config_dir(config_dir)
    }
}

/// Accept only an absolute, non-symlink directory with no user configuration.
fn is_fresh_empty_directory(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_dir() {
        return false;
    }
    fs::read_dir(path).is_ok_and(|entries| entries.count() == 0)
}

/// Accept only the current-repository facts used by cache writer trust.
fn is_official_repository_api_query(
    tools: &[PinnedTool],
    program: &OsStr,
    args: &[std::ffi::OsString],
) -> bool {
    let [api, endpoint, hostname_flag, hostname, jq_flag, selector] = args else {
        return false;
    };
    let Some(endpoint) = endpoint.to_str() else {
        return false;
    };
    tools == [PinnedTool::Gh]
        && program == OsStr::new("gh")
        && api.as_os_str() == OsStr::new("api")
        && endpoint.starts_with("repos/")
        && !endpoint.contains('?')
        && !endpoint.contains('#')
        && hostname_flag.as_os_str() == OsStr::new("--hostname")
        && hostname.as_os_str() == OsStr::new("github.com")
        && jq_flag.as_os_str() == OsStr::new("--jq")
        && (selector.as_os_str() == OsStr::new(".default_branch")
            || selector.as_os_str() == OsStr::new(".protected"))
}
