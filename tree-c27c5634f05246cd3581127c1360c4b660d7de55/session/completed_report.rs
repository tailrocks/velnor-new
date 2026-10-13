//! Supported completed-session reports, independent of the live event ledger.
//!
//! A report appears as `<session UUID>.json` only after the owning workload
//! has ended and all session counters have been collected. Files are never
//! replaced. Command roles contain no command arguments or environment values.

use eyre::{Context, Result, bail};
use serde::Serialize;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub const SESSION_ID_ENV: &str = "MBX_REPORT_SESSION_ID";
pub const ROOT_SESSION_ID_ENV: &str = "MBX_REPORT_ROOT_SESSION_ID";
/// The owning session's parent, when this is a nested owner.
pub const PARENT_SESSION_ID_ENV: &str = "MBX_REPORT_PARENT_SESSION_ID";
/// Optional public caller token used only to group independent session roots.
/// It supplies no authority about source, package ownership, or cache content.
pub const CORRELATION_ID_ENV: &str = "MBX_REPORT_CORRELATION_ID";
pub const REPORT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandRole {
    CargoBuild,
    CargoCheck,
    CargoTest,
    CargoRun,
    CargoDoc,
    Exec,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionIdentity {
    pub session_id: String,
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub command_role: CommandRole,
    pub caller_correlation: Option<String>,
}

impl SessionIdentity {
    /// Start an independent session, or a child of an explicitly propagated
    /// identity. Malformed or incomplete inheritance is an error.
    pub fn new(
        role: CommandRole,
        parent: Option<&str>,
        root: Option<&str>,
        correlation: Option<&str>,
    ) -> Result<Self> {
        match (parent, root) {
            (None, None) => {}
            (Some(parent), Some(root)) if valid_uuid(parent) && valid_uuid(root) => {}
            _ => bail!("invalid completed-report parent/root session identity"),
        }
        validate_correlation(correlation)?;
        let session_id = new_uuid();
        Ok(Self {
            root_session_id: root.unwrap_or(&session_id).to_owned(),
            session_id,
            parent_session_id: parent.map(str::to_owned),
            command_role: role,
            caller_correlation: correlation.map(str::to_owned),
        })
    }

    /// Values propagated to launched children. A child uses the current
    /// session as its parent and retains the original root.
    pub fn child_environment(&self) -> Vec<(&'static str, &str)> {
        let mut environment: Vec<(&'static str, &str)> = vec![
            (SESSION_ID_ENV, &self.session_id),
            (ROOT_SESSION_ID_ENV, &self.root_session_id),
        ];
        if let Some(parent) = &self.parent_session_id {
            environment.push((PARENT_SESSION_ID_ENV, parent));
        }
        if let Some(correlation) = &self.caller_correlation {
            environment.push((CORRELATION_ID_ENV, correlation));
        }
        environment
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_correlation(self.caller_correlation.as_deref())?;
        if !valid_uuid(&self.session_id) || !valid_uuid(&self.root_session_id) {
            bail!("invalid completed-report session identity");
        }
        match &self.parent_session_id {
            Some(parent)
                if valid_uuid(parent)
                    && parent != &self.session_id
                    && self.session_id != self.root_session_id =>
            {
                Ok(())
            }
            None if self.session_id == self.root_session_id => Ok(()),
            _ => bail!("invalid completed-report session ancestry"),
        }
    }
}

pub(crate) fn validate_correlation(correlation: Option<&str>) -> Result<()> {
    if correlation.is_some_and(|value| {
        value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
    }) {
        bail!("completed-report correlation must contain 1..128 public ASCII token characters");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadOutcome {
    Succeeded,
    Failed,
    Terminated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkloadResult {
    pub outcome: WorkloadOutcome,
    /// None means the operating system reported termination without a code.
    pub exit_code: Option<i32>,
}

impl From<std::process::ExitStatus> for WorkloadResult {
    fn from(status: std::process::ExitStatus) -> Self {
        Self {
            outcome: if status.success() {
                WorkloadOutcome::Succeeded
            } else if status.code().is_some() {
                WorkloadOutcome::Failed
            } else {
                WorkloadOutcome::Terminated
            },
            exit_code: status.code(),
        }
    }
}

/// Schema version changes only when the consumer contract changes. Statistics
/// are supplied directly by the owning agent, never reconstructed from logs.
#[derive(Debug, Serialize)]
pub struct CompletedReport<'a, T: Serialize> {
    pub schema_version: u8,
    pub completed: bool,
    pub mbx_version: &'static str,
    pub source_base_version: &'static str,
    pub identity: &'a SessionIdentity,
    pub workload: WorkloadResult,
    pub statistics: &'a T,
}

/// Publish exactly once in an owned, private directory. Serialization and
/// staging failures leave no completed JSON file. A duplicate identity fails
/// rather than overwriting the existing report.
pub fn publish<T: Serialize>(
    directory: &Path,
    identity: &SessionIdentity,
    workload: WorkloadResult,
    statistics: &T,
) -> Result<PathBuf> {
    identity.validate()?;
    let report = CompletedReport {
        schema_version: REPORT_SCHEMA_VERSION,
        completed: true,
        mbx_version: crate::version::VERSION,
        source_base_version: crate::version::SOURCE_BASE_VERSION,
        identity,
        workload,
        statistics,
    };
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    prepare_directory(directory)?;
    let destination = directory.join(format!("{}.json", identity.session_id));
    let mut temporary = tempfile::Builder::new()
        .prefix(".mbx-report-")
        .tempfile_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file_mut().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o400))?;
    }
    temporary
        .persist_noclobber(&destination)
        .map_err(|error| error.error)
        .wrap_err_with(|| {
            format!(
                "failed to publish completed report {}",
                destination.display()
            )
        })?;
    Ok(destination)
}

pub(crate) fn prepare_directory(directory: &Path) -> Result<()> {
    if !cfg!(unix) {
        bail!("completed-report private directory policy is unqualified on this platform");
    }
    if !directory.is_absolute() {
        bail!("completed-report directory must be absolute");
    }
    reject_aliases(directory)?;
    if !directory.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(directory)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir(directory)?;
    }
    reject_aliases(directory)?;
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() {
        bail!("completed-report destination is not a directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // A private directory created by tempfile supplies our effective owner
        // without consulting spoofable environment variables or unsafe FFI.
        let owner_probe = tempfile::tempdir()?;
        let owner = std::fs::metadata(owner_probe.path())?.uid();
        if metadata.uid() != owner || metadata.mode() & 0o077 != 0 {
            bail!("completed-report directory must be owned by this user and private");
        }
    }
    Ok(())
}

fn reject_aliases(directory: &Path) -> Result<()> {
    let mut prefix = PathBuf::new();
    for component in directory.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            bail!("completed-report directory must not contain path aliases");
        }
        prefix.push(component);
        match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("completed-report directory must not traverse symlinks");
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if prefix.as_os_str() != directory.as_os_str() {
        bail!("completed-report directory must not contain path aliases");
    }
    Ok(())
}

fn new_uuid() -> String {
    let mut bytes = rand::random::<[u8; 16]>();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub(crate) fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && value.as_bytes()[14] == b'4'
        && matches!(value.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
}

#[cfg(test)]
#[path = "completed_report_tests.rs"]
mod tests;
