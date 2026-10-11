//! Guarded execution for action-owned MBX metadata requests.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{IsolatedCommand, ProcessOutput};
use crate::error::MiseError;

use super::PinnedToolExec;

const MBX_PROGRAM: &str = "mbx";
const MBX_VERSION_ARGUMENT: &str = "--version";

/// Metadata execution whose action-owned MBX authority is checked before
/// every metadata process is spawned.
///
/// The wrapped commands are intentionally private: callers may inspect the
/// metadata argv and environment or add the same validated overlays to both
/// invocations, but cannot execute metadata without the version preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataCommand {
    version_probe: IsolatedCommand,
    metadata: IsolatedCommand,
    catalog: ToolCatalog,
}

impl MetadataCommand {
    pub(crate) fn new(
        catalog: &ToolCatalog,
        full_mbx_argv: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        let (program, payload_args) = split_mbx_argv(full_mbx_argv)?;
        let tools = vec![PinnedTool::Rust];
        let version_probe = PinnedToolExec::new_action_owned_mbx(
            tools.clone(),
            &program,
            vec![OsString::from(MBX_VERSION_ARGUMENT)],
        )?
        .command(catalog)?;
        let metadata = PinnedToolExec::new_action_owned_mbx(tools, &program, payload_args)?
            .command(catalog)?;
        Ok(Self {
            version_probe,
            metadata,
            catalog: catalog.clone(),
        })
    }

    /// Metadata command argv, for inspection only.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        self.metadata.argv()
    }

    /// Program used by the Mise invocation.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        self.metadata.program()
    }

    /// Working-directory override shared by the probe and metadata command.
    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.metadata.cwd()
    }

    /// Read-only environment overlay applied to the metadata command.
    #[must_use]
    pub fn full_env(&self) -> Vec<(OsString, OsString)> {
        self.metadata.full_env()
    }

    /// Whether the metadata command disables Mise's implicit installation.
    #[must_use]
    pub fn disables_auto_install(&self) -> bool {
        self.metadata.disables_auto_install()
    }

    /// Set one working directory for both preflight and metadata invocations.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.version_probe = self.version_probe.with_cwd(cwd.clone());
        self.metadata = self.metadata.with_cwd(cwd);
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
            self.version_probe = self.version_probe.with_env(&pair)?;
            self.metadata = self.metadata.with_env(&pair)?;
        }
        Ok(self)
    }

    /// Verify action-owned MBX, then run the metadata request.
    ///
    /// Both processes use the same Rust selector, Mise isolation policy,
    /// working directory, and validated environment additions. The probe's
    /// output is only interpreted as a strict version line; diagnostics never
    /// copy arbitrary probe output into an error.
    ///
    /// # Errors
    ///
    /// Returns a spawn/exit/UTF-8/version error when the exact action-owned
    /// MBX cannot be verified, and otherwise the metadata process result.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        self.verify_action_mbx()?;
        self.metadata.run()
    }

    fn verify_action_mbx(&self) -> Result<(), MiseError> {
        let output = self.version_probe.run()?;
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

fn split_mbx_argv(argv: Vec<OsString>) -> Result<(OsString, Vec<OsString>), MiseError> {
    let mut argv = argv.into_iter();
    let Some(program) = argv.next() else {
        return Err(MiseError::EmptyCommand {
            program: "mbx".to_owned(),
        });
    };
    if program != OsStr::new(MBX_PROGRAM) {
        return Err(MiseError::ForbiddenPayload {
            program: program.to_string_lossy().into_owned(),
            reason: "metadata_requires_mbx_payload".to_owned(),
        });
    }
    Ok((program, argv.collect()))
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
