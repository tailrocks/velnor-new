//! Native repository object-format admission and source-config binding.
//! The private index parser establishes a typed OID width; this module asks
//! Git through a fixed read-only query and binds the relevant config files.
//! Includes remain outside this snapshot; same-user mutation is an honest
//! filesystem race boundary.

use super::repository::RepositoryContext;
use crate::MiseError;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::CancelHandle;
use super::super::native::NativeGitBinding;
#[path = "command_git_index_config.rs"]
pub(in crate::command::git) mod config;
use config::{ConfigSnapshot, DirectoryBinding};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use config::{canonical_directory, invalid, snapshot, verify_directory};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

/// The only object formats admitted by a validated private index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::command::git) enum IndexObjectFormat {
    /// Git's twenty-byte object identifiers.
    Sha1,
    /// Git's thirty-two-byte object identifiers.
    Sha256,
}

impl IndexObjectFormat {
    /// Convert the parser's closed width result into a typed format.
    pub(in crate::command::git) fn from_width(width: usize) -> Option<Self> {
        match width {
            20 => Some(Self::Sha1),
            32 => Some(Self::Sha256),
            _ => None,
        }
    }
}

/// Closed result of one native object-format query and its source bindings.
#[derive(Debug)]
pub(in crate::command::git) struct NativeFormatResult<'a> {
    format: IndexObjectFormat,
    common: &'a CommonRepository,
    common_config: ConfigSnapshot,
    worktree_config: ConfigSnapshot,
}

impl NativeFormatResult<'_> {
    pub(in crate::command::git) fn format(&self) -> IndexObjectFormat {
        self.format
    }

    /// Verify config routing and source config bytes before the real diff.
    pub(in crate::command::git) fn verify_unchanged(&self) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.common.verify()?;
            self.common_config.verify()?;
            self.worktree_config.verify()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(unsupported_io())
        }
    }
}

/// Captured before private writes so common metadata can be excluded too.
#[derive(Debug)]
pub(in crate::command::git) struct CommonRepository {
    path: PathBuf,
    binding: DirectoryBinding,
    marker: ConfigSnapshot,
}

impl CommonRepository {
    pub(in crate::command::git) fn capture(context: &RepositoryContext) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let (path, binding, marker) = resolve_common_gitdir(context)?;
            Ok(Self {
                path,
                binding,
                marker,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = context;
            Err(unsupported_io())
        }
    }

    pub(in crate::command::git) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(in crate::command::git) fn verify(&self) -> io::Result<()> {
        verify_directory(&self.path, &self.binding, "common_gitdir")?;
        self.marker.verify()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(in crate::command::git) fn verify(&self) -> io::Result<()> {
        Err(unsupported_io())
    }
}

/// Probe Git's storage format and capture the config files used by discovery.
pub(in crate::command::git) fn probe<'a>(
    native: &NativeGitBinding,
    context: &RepositoryContext,
    common: &'a CommonRepository,
    expected: Option<IndexObjectFormat>,
    cap: usize,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<NativeFormatResult<'a>, MiseError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        probe_supported(native, context, common, expected, cap, timeout, cancel)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (native, context, common, expected, cap, timeout, cancel);
        Err(unsupported())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn probe_supported<'a>(
    native: &NativeGitBinding,
    context: &RepositoryContext,
    common: &'a CommonRepository,
    expected: Option<IndexObjectFormat>,
    cap: usize,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<NativeFormatResult<'a>, MiseError> {
    context
        .verify_binding()
        .map_err(|error| spawn_failed(&error))?;
    common.verify().map_err(|error| spawn_failed(&error))?;
    let common_config = snapshot(
        common.path.join("config"),
        MAX_CONFIG_BYTES,
        "common_config",
    )
    .map_err(|error| spawn_failed(&error))?;
    let worktree_config = snapshot(
        context.gitdir().join("config.worktree"),
        MAX_CONFIG_BYTES,
        "worktree_config",
    )
    .map_err(|error| spawn_failed(&error))?;
    let query = native.command(vec![
        "-c".into(),
        "core.splitIndex=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "core.hooksPath=/dev/null".into(),
        "rev-parse".into(),
        "--show-object-format".into(),
    ]);
    let mut command = query.command()?;
    context
        .apply(&mut command)
        .map_err(|error| spawn_failed(&error))?;
    command.env("GIT_NO_LAZY_FETCH", "1");
    let output = super::super::super::process::run(&query, command, cap, timeout, cancel).result?;
    if !output.success {
        return Err(spawn_failed(&io::Error::other(
            "object_format_query_failed",
        )));
    }
    let actual = match output.stdout.as_slice() {
        b"sha1\n" => IndexObjectFormat::Sha1,
        b"sha256\n" => IndexObjectFormat::Sha256,
        _ => {
            return Err(MiseError::InvalidStepInput {
                field: "git_object_format".to_owned(),
                value: "native_object_format_invalid".to_owned(),
            });
        }
    };
    if expected.is_some_and(|expected| expected != actual) {
        return Err(MiseError::InvalidStepInput {
            field: "git_object_format".to_owned(),
            value: "native_index_object_format_mismatch".to_owned(),
        });
    }
    let result = NativeFormatResult {
        format: actual,
        common,
        common_config,
        worktree_config,
    };
    result
        .verify_unchanged()
        .map_err(|error| spawn_failed(&error))?;
    Ok(result)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_common_gitdir(
    context: &RepositoryContext,
) -> io::Result<(PathBuf, DirectoryBinding, ConfigSnapshot)> {
    let marker_path = context.gitdir().join("commondir");
    let marker = snapshot(marker_path.clone(), fs::MAX_POINTER_BYTES, "commondir")?;
    let Some(bytes) = marker.bytes.as_deref() else {
        let (path, binding) = canonical_directory(context.gitdir(), "common_gitdir")?;
        return Ok((path, binding, marker));
    };
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|_| invalid("commondir", "invalid_utf8"))?;
    let mut line = text.as_str();
    if line.ends_with('\n') {
        line = &line[..line.len() - 1];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
    }
    if line.is_empty() || line.contains(['\r', '\n', '\0']) {
        return Err(invalid("commondir", "invalid_target"));
    }
    let unresolved = PathBuf::from(line);
    let unresolved = if unresolved.is_absolute() {
        unresolved
    } else {
        context.gitdir().join(unresolved)
    };
    let (path, binding) = canonical_directory(&unresolved, "common_gitdir")?;
    Ok((path, binding, marker))
}

fn spawn_failed(error: &io::Error) -> MiseError {
    MiseError::SpawnFailed {
        program: "git".to_owned(),
        message: error.to_string(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> MiseError {
    MiseError::SpawnFailed {
        program: "git".to_owned(),
        message: "private_git_repository_format:unsupported_platform".to_owned(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported_io() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "private_git_repository_format:unsupported_platform",
    )
}
