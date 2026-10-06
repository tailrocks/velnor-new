//! Private admission for the one native Git executable used by discovery.
//!
//! Resolution trusts only the process ambient `PATH`, resolves one physical
//! executable, and records its bounded SHA-256 bytes.  Every later command
//! must use [`NativeGitBinding::bind`]; the existing process owner verifies
//! the typed executable immediately before spawning.  Rust's
//! portable `Command` API cannot execute a retained file descriptor; a same
//! UID replacement after that final check remains an explicit race boundary.
//!
//! The version allowlist is intentionally exact.  The primary Git manuals
//! document the admission controls in both audited tags:
//! * <https://github.com/git/git/blob/v2.54.0/Documentation/git.adoc#L168-L177>
//! * <https://github.com/git/git/blob/v2.56.0/Documentation/git.adoc#L169-L178>
//! * <https://github.com/git/git/blob/v2.54.0/Documentation/config/core.adoc#L58-L115>
//! * <https://github.com/git/git/blob/v2.56.0/Documentation/config/core.adoc#L58-L115>
//! * <https://github.com/git/git/blob/v2.54.0/Documentation/config/core.adoc#L474-L494>
//! * <https://github.com/git/git/blob/v2.56.0/Documentation/config/core.adoc#L474-L494>

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use sha2::{Digest, Sha256};

use super::super::{CancelHandle, EnvPolicy, IsolatedCommand};
use crate::MiseError;

/// Bound for one physically resolved native executable read.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
/// Bound for the fixed `git --version` response.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const VERSION_OUTPUT_LIMIT_BYTES: usize = 4096;
/// Bound for the fixed version admission child.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Fixed version probe argv.  No repository or caller arguments enter it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const VERSION_ARGUMENTS: [&str; 1] = ["--version"];
/// Exact Git versions admitted by the audited source tags.
const SUPPORTED_VERSIONS: [&str; 2] = ["2.54.0", "2.56.0"];
/// Controls applied to every command bound to this executable.
const FIXED_GIT_ENV: [(&str, &str); 2] = [("GIT_NO_LAZY_FETCH", "1"), ("GIT_OPTIONAL_LOCKS", "0")];

/// One admitted physical Git executable and its observed bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeGitBinding {
    executable: ResolvedGitExecutable,
    template: IsolatedCommand,
}

/// Unforgeable physical executable selector; only this module constructs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::command) struct ResolvedGitExecutable {
    path: PathBuf,
    sha256: [u8; 32],
}

impl ResolvedGitExecutable {
    pub(in crate::command) fn path(&self) -> &Path {
        &self.path
    }

    pub(in crate::command) fn verify(&self) -> Result<(), MiseError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if read_digest(&self.path)? == self.sha256 {
                return Ok(());
            }
            Err(invalid("git_executable_changed"))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(invalid("git_native_unsupported_platform"))
        }
    }
}

impl NativeGitBinding {
    /// Resolve ambient Git, admit its exact native version, and bind its bytes.
    ///
    /// No caller path is accepted.  The caller's timeout and cancellation
    /// bound the fixed version probe; its output has an independent cap.
    ///
    /// # Errors
    ///
    /// Returns a typed input error for missing/unsafe Git or unsupported
    /// vendor builds, and a typed spawn error from the fixed version probe.
    pub(super) fn discover(
        owner: &IsolatedCommand,
        cap: usize,
        timeout: Duration,
        cancel: &CancelHandle,
    ) -> Result<Self, MiseError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Self::discover_supported(owner, cap, timeout, cancel)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, cap, timeout, cancel);
            Err(invalid("git_native_unsupported_platform"))
        }
    }

    /// Build a logical Discovery Git command backed by this physical path.
    ///
    /// The central command owner keeps the logical program name `git` for
    /// policy and diagnostics; the private resolved-program slot selects the
    /// canonical executable at spawn.
    pub(super) fn command(&self, args: Vec<OsString>) -> IsolatedCommand {
        {
            let mut command = self.template.clone();
            command.args = args;
            command.with_native_git(&self.executable)
        }
    }

    /// Attach this binding to an already-admitted logical Discovery command.
    ///
    /// # Errors
    ///
    /// Rejects accidental rebinding of another program or environment policy.
    pub(super) fn bind(&self, command: IsolatedCommand) -> Result<IsolatedCommand, MiseError> {
        if command != self.template
            || command.program != "git"
            || command.policy != EnvPolicy::Discovery
        {
            return Err(invalid("git_native_command_identity"));
        }
        Ok(command.with_native_git(&self.executable))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn discover_supported(
        owner: &IsolatedCommand,
        cap: usize,
        timeout: Duration,
        cancel: &CancelHandle,
    ) -> Result<Self, MiseError> {
        if owner.program != "git" || owner.policy != EnvPolicy::Discovery {
            return Err(invalid("git_native_command_identity"));
        }
        let (path, sha256) = resolve_ambient_git()?;
        let executable = ResolvedGitExecutable { path, sha256 };
        let mut query = owner.clone().with_native_git(&executable);
        query.args = VERSION_ARGUMENTS.iter().map(OsString::from).collect();
        let command = query.command()?;
        let cap = cap.min(VERSION_OUTPUT_LIMIT_BYTES);
        let before = read_digest(executable.path())?;
        if before != sha256 {
            return Err(invalid("git_executable_changed"));
        }
        let output =
            super::super::process::run(&query, command, cap, timeout.min(VERSION_TIMEOUT), cancel)
                .result?;
        let actual =
            read_digest(executable.path()).map_err(|_| invalid("git_executable_changed"))?;
        if actual != sha256 {
            return Err(invalid("git_executable_changed"));
        }
        admit_version(output.success, &output.stdout, &output.stderr)?;
        Ok(Self {
            executable,
            template: owner.clone(),
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_ambient_git() -> Result<(PathBuf, [u8; 32]), MiseError> {
    let path = std::env::var_os("PATH").ok_or_else(|| invalid("git_path_missing"))?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("git");
        let metadata = match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(invalid("git_path_entry_unreadable")),
        };
        if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
            continue;
        }
        let executable = match candidate.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(invalid("git_executable_unresolvable")),
        };
        let Ok(metadata) = std::fs::symlink_metadata(&executable) else {
            return Err(invalid("git_executable_unreadable"));
        };
        if !metadata.file_type().is_file() || !is_executable(&metadata) {
            continue;
        }
        let sha256 = read_digest(&executable)?;
        return Ok((executable, sha256));
    }
    Err(invalid("git_executable_not_found"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_digest(path: &Path) -> Result<[u8; 32], MiseError> {
    let bytes = super::index::fs::read_checked(path, MAX_EXECUTABLE_BYTES, "git_executable")
        .map_err(|_| invalid("git_executable_unreadable"))?;
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&Sha256::digest(&bytes));
    Ok(digest)
}

fn admit_version(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<(), MiseError> {
    if !success {
        return Err(invalid("git_version_probe_failed"));
    }
    if !stderr.is_empty() {
        return Err(invalid("git_version_output_invalid"));
    }
    match stdout {
        b"git version 2.54.0\n" | b"git version 2.56.0\n" => Ok(()),
        bytes if bytes.starts_with(b"git version ") => {
            let suffix = &bytes[b"git version ".len()..];
            if SUPPORTED_VERSIONS
                .iter()
                .any(|version| suffix.starts_with(version.as_bytes()))
            {
                Err(invalid("git_version_vendor_unsupported"))
            } else {
                Err(invalid("git_version_unsupported"))
            }
        }
        _ => Err(invalid("git_version_output_invalid")),
    }
}

fn invalid(value: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_native".to_owned(),
        value: value.to_owned(),
    }
}

impl IsolatedCommand {
    /// Select a canonical executable without changing the logical Git owner.
    ///
    /// `resolved_git` is private to the central command wrapper.  This
    /// method is the only Git binding write; no caller path reaches it.
    fn with_native_git(mut self, executable: &ResolvedGitExecutable) -> Self {
        self.resolved_git = Some(executable.clone());
        self.extra_env.retain(|(key, _)| key != "PATH");
        for (name, value) in FIXED_GIT_ENV {
            self.extra_env
                .retain(|(key, _)| key != &OsString::from(name));
            self.extra_env
                .push((OsString::from(name), OsString::from(value)));
        }
        self
    }
}
