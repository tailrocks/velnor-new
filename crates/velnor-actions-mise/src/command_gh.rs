//! Internal GitHub CLI endpoint configuration for fixed API reads.

use std::ffi::OsString;
use std::path::Path;

use super::{EnvPolicy, IsolatedCommand};
use crate::error::MiseError;

impl IsolatedCommand {
    /// Set a Velnor-owned GitHub CLI config directory for fixed API reads.
    ///
    /// `GH_CONFIG_DIR` is normally reserved and stripped. This crate-only
    /// exception is limited to baseline commands and absolute paths; the
    /// typed pinned-exec wrapper validates the request before calling it.
    pub(crate) fn with_internal_gh_config_dir(mut self, path: &Path) -> Result<Self, MiseError> {
        if self.policy != EnvPolicy::Baseline || !path.is_absolute() {
            return Err(MiseError::InvalidStepInput {
                field: "GH_CONFIG_DIR".to_owned(),
                value: "requires_absolute_baseline_path".to_owned(),
            });
        }
        self.extra_env
            .push((OsString::from("GH_CONFIG_DIR"), path.as_os_str().to_owned()));
        Ok(self)
    }
}
