//! Guarded execution for action-owned MBX metadata requests.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{ActionOwnedMbxPath, IsolatedCommand, NO_AUTO_INSTALL_ENV, ProcessOutput};
use crate::error::MiseError;

use super::PinnedToolExec;

/// Metadata execution whose action-owned MBX authority is checked before
/// every metadata process is spawned.
///
/// The wrapped tool executions are intentionally private. `run` resolves the
/// action-provided executable once from the final filtered environment, uses
/// it for the version check, and then reuses it for metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataCommand {
    version_probe: PinnedToolExec,
    metadata: PinnedToolExec,
    catalog: ToolCatalog,
    cwd: Option<PathBuf>,
    extra_env: Vec<(OsString, OsString)>,
}

impl MetadataCommand {
    pub(crate) fn new(
        catalog: &ToolCatalog,
        metadata_args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        if metadata_args.first().map(OsString::as_os_str) != Some(OsStr::new("metadata")) {
            return Err(MiseError::ForbiddenPayload {
                program: "mbx".to_owned(),
                reason: "metadata_requires_metadata_subcommand".to_owned(),
            });
        }
        let tools = vec![PinnedTool::Rust];
        let version_probe = PinnedToolExec::new_action_owned_mbx_version_probe(tools.clone())?;
        let metadata = PinnedToolExec::new_action_owned_mbx(tools, metadata_args)?;
        Ok(Self {
            version_probe,
            metadata,
            catalog: catalog.clone(),
            cwd: None,
            extra_env: Vec::new(),
        })
    }

    /// Portable metadata argv, for inspection only. Runtime replaces `mbx`
    /// with the one absolute action-owned executable before invoking Mise.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        self.metadata.argv(&self.catalog)
    }

    /// Program used by the Mise invocation.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        OsStr::new("mise")
    }

    /// Working-directory override shared by the probe and metadata command.
    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Read-only environment overlay applied to both commands.
    #[must_use]
    pub fn full_env(&self) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(
            NO_AUTO_INSTALL_ENV
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        );
        env.extend(self.extra_env.iter().cloned());
        env
    }

    /// Whether the metadata command disables Mise's implicit installation.
    #[must_use]
    pub const fn disables_auto_install(&self) -> bool {
        true
    }

    /// Set one working directory for both preflight and metadata invocations.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Add the same validated environment entries to both invocations.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for a reserved environment
    /// name or an explicit Rustup selector that differs from the catalog pin.
    /// No raw command can be extracted from this wrapper.
    pub fn with_env(mut self, extra: &[(OsString, OsString)]) -> Result<Self, MiseError> {
        let expected_toolchain = self.catalog.rustup_toolchain();
        let probe = self.version_probe.command_template(&self.catalog)?;
        let metadata = self.metadata.command_template(&self.catalog)?;
        for (key, value) in extra {
            if key == crate::command::RUSTUP_TOOLCHAIN_ENV
                && value.as_os_str() != OsStr::new(expected_toolchain.as_str())
            {
                return Err(MiseError::InvalidStepInput {
                    field: crate::command::RUSTUP_TOOLCHAIN_ENV.to_owned(),
                    value: "must_match_catalog_rust_pin".to_owned(),
                });
            }
            let pair = [(key.clone(), value.clone())];
            probe.clone().with_env(&pair)?;
            metadata.clone().with_env(&pair)?;
        }
        self.extra_env.extend(extra.iter().cloned());
        Ok(self)
    }

    /// Verify action-owned MBX, then run the metadata request.
    ///
    /// Both processes use the same Rust selector, Mise isolation policy,
    /// working directory, validated environment additions, and resolved MBX
    /// path. Probe output is only interpreted as a strict version line; errors
    /// do not copy arbitrary probe output into diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a spawn/exit/UTF-8/version error when the exact action-owned
    /// MBX cannot be verified, and otherwise the metadata process result.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        let metadata_template = self.command_template(&self.metadata)?;
        let mbx = metadata_template.resolve_action_owned_mbx()?;
        let output = self.command_with_path(&self.version_probe, &mbx)?.run()?;
        self.verify_action_mbx(&output)?;
        self.command_with_path(&self.metadata, &mbx)?.run()
    }

    fn command_template(&self, exec: &PinnedToolExec) -> Result<IsolatedCommand, MiseError> {
        let mut command = exec.command_template(&self.catalog)?;
        if let Some(cwd) = &self.cwd {
            command = command.with_cwd(cwd.clone());
        }
        command.with_env(&self.extra_env)
    }

    fn command_with_path(
        &self,
        exec: &PinnedToolExec,
        path: &ActionOwnedMbxPath,
    ) -> Result<IsolatedCommand, MiseError> {
        let mut command = exec.command_with_action_owned_path(&self.catalog, path)?;
        if let Some(cwd) = &self.cwd {
            command = command.with_cwd(cwd.clone());
        }
        command.with_env(&self.extra_env)
    }

    fn verify_action_mbx(&self, output: &ProcessOutput) -> Result<(), MiseError> {
        if !output.success {
            return Err(MiseError::NonZeroExit {
                program: "mbx --version".to_owned(),
                code: output.code,
                stderr: String::new(),
            });
        }
        let stdout = output.stdout_text("mbx --version")?;
        let version = strict_mbx_version(&stdout).unwrap_or("<malformed-output>");
        self.catalog.reconcile_action_mbx(version)
    }
}

fn strict_mbx_version(stdout: &str) -> Option<&str> {
    let line = stdout.strip_suffix('\n')?;
    let version = line.strip_prefix("mbx ")?;
    if version.is_empty() || version.contains('\r') || version.contains('\n') {
        return None;
    }
    Some(version)
}

#[cfg(test)]
mod tests {
    use super::strict_mbx_version;

    #[test]
    fn version_parser_requires_one_canonical_line() {
        assert_eq!(strict_mbx_version("mbx 1.23.0\n"), Some("1.23.0"));
        for malformed in [
            "mbx 1.23.0",
            "mbx 1.23.0\nextra\n",
            "mbx 1.23.0\r\n",
            "other 1.23.0\n",
            "mbx \n",
        ] {
            assert_eq!(strict_mbx_version(malformed), None, "{malformed:?}");
        }
    }
}
