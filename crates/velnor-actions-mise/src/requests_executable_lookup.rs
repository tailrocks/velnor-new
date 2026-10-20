//! This module finds the executable for one exact Mise tool version.

use crate::command::{IsolatedCommand, ProcessOutput};
use crate::error::MiseError;
use rustix::fs::{FileType, Mode, OFlags, openat};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const LOOKUP_OUTPUT_LIMIT: usize = 4096;
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
const RUST_COMPONENTS: [&str; 5] = ["rustc", "cargo", "cargo-clippy", "cargo-fmt", "rustfmt"];
const EXECUTABLE_MAX_BYTES: u64 = 256 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";
const CATALOG_EXECUTABLES: [(&str, &str, &str); 8] = [
    ("mr-boxington", "mr-boxington", "mbx"),
    ("gh", "gh", "gh"),
    ("actionlint", "actionlint", "actionlint"),
    ("shellcheck", "shellcheck", "shellcheck"),
    ("zizmor", "zizmor", "zizmor"),
    (
        "aqua:nextest-rs/nextest/cargo-nextest",
        "nextest",
        "cargo-nextest",
    ),
    ("opentofu", "opentofu", "tofu"),
    ("release-plz", "release-plz", "release-plz"),
];

/// Resolve one binary from an exact Mise tool selector.
///
/// The command returns a path only.
/// The caller must confirm that the path is within an allowed directory.
/// The caller must confirm that the path names a regular file owned by the current user.
/// The caller must then hash the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseExecutableLookup {
    command: IsolatedCommand,
}

/// Executable path and SHA-256 digest from the selected tool installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseExecutableObservation {
    /// Resolved executable path reported by Mise or Rustup.
    pub path: PathBuf,
    /// SHA-256 digest of the bytes read from the opened regular file.
    pub sha256: String,
}

impl MiseExecutableLookup {
    /// Resolve with `mise which <binary> --tool <selector>`.
    ///
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an incorrect selector or
    /// executable name.
    pub fn mise(selector: &str, executable: &str) -> Result<Self, MiseError> {
        let (tool, version) = validate_selector(selector)?;
        let (_, version_tool, _) = CATALOG_EXECUTABLES
            .iter()
            .find(|(selector_tool, _, _)| *selector_tool == tool)
            .ok_or_else(|| invalid("selector"))?;
        crate::catalog::PinnedTool::from_tool_name(version_tool)?;
        crate::catalog::validate_concrete_exact_version(version_tool, version)?;
        validate_catalog_executable(tool, executable)?;
        Ok(Self {
            command: IsolatedCommand::mise_which(executable, selector),
        })
    }

    /// Resolve a Rust component from the actual selected Rustup toolchain.
    /// This avoids hashing the Rustup shim returned by `mise which`.
    ///
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for a non-Rust selector,
    /// invalid toolchain pin, or unsupported Rust component.
    pub fn rust_toolchain(selector: &str, component: &str) -> Result<Self, MiseError> {
        let version = selector
            .strip_prefix("rust@")
            .ok_or_else(|| invalid("selector"))?;
        crate::catalog::validate_concrete_exact_version("rust", version)?;
        if !RUST_COMPONENTS.contains(&component) {
            return Err(invalid("rust_component"));
        }
        let payload = [
            OsString::from("rustup"),
            OsString::from("which"),
            OsString::from("--toolchain"),
            OsString::from(version),
            OsString::from(component),
        ];
        Ok(Self {
            command: IsolatedCommand::mise_exec(&[selector.to_owned()], &payload)?,
        })
    }

    /// Isolated command for execution through the central subprocess boundary.
    #[must_use]
    pub fn command(&self) -> &IsolatedCommand {
        &self.command
    }

    /// Run the bounded lookup and parse its single absolute path.
    ///
    /// # Errors
    /// Returns a command error or rejects output that is too large, does not contain one path, or gives a relative path.
    /// This method does not authenticate or hash the returned file.
    pub fn resolve_path(&self) -> Result<PathBuf, MiseError> {
        let output = self
            .command
            .run_bounded(LOOKUP_OUTPUT_LIMIT, LOOKUP_TIMEOUT)?;
        parse_path_output(output)
    }

    /// Resolve and hash a regular executable beneath its trusted install root.
    ///
    /// The code opens each path part from a fixed directory and does not follow symbolic links.
    /// The opened file must belong to the current
    /// user, have an executable mode bit, and remain below `install_root`.
    ///
    /// # Errors
    /// Returns a `MiseError` if the lookup fails or a path part is unsafe.
    /// The executable must belong to the current user and must not exceed the size limit.
    pub fn resolve_and_hash(
        &self,
        install_root: &Path,
    ) -> Result<MiseExecutableObservation, MiseError> {
        let path = self.resolve_path()?;
        let sha256 = hash_owned_executable(&path, install_root)?;
        Ok(MiseExecutableObservation { path, sha256 })
    }
}

fn validate_selector(selector: &str) -> Result<(&str, &str), MiseError> {
    let Some((tool, version)) = selector.split_once('@') else {
        return Err(invalid("selector"));
    };
    if tool.is_empty()
        || version.is_empty()
        || selector.len() > 256
        || selector.matches('@').count() != 1
        || !selector
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:/+@".contains(&byte))
    {
        return Err(invalid("selector"));
    }
    Ok((tool, version))
}

fn validate_catalog_executable(tool: &str, executable: &str) -> Result<(), MiseError> {
    if !CATALOG_EXECUTABLES
        .iter()
        .any(|(selector_tool, _, expected)| *selector_tool == tool && *expected == executable)
    {
        return Err(invalid("executable"));
    }
    Ok(())
}

fn parse_path_output(output: ProcessOutput) -> Result<PathBuf, MiseError> {
    output.require_success("mise-executable-lookup")?;
    if output.stdout.len() > LOOKUP_OUTPUT_LIMIT {
        return Err(invalid("output_oversize"));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| invalid("output_encoding"))?;
    let path = text.strip_suffix('\n').unwrap_or(&text);
    if path.is_empty() || path.contains(['\n', '\r', '\0']) {
        return Err(invalid("output_shape"));
    }
    let path = Path::new(path);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("output_path"));
    }
    Ok(path.to_path_buf())
}

fn hash_owned_executable(path: &Path, install_root: &Path) -> Result<String, MiseError> {
    if !install_root.is_absolute()
        || install_root
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("install_root"));
    }
    let root = install_root
        .canonicalize()
        .map_err(|_| invalid("install_root"))?;
    let reported_path = path.to_path_buf();
    let path = path.canonicalize().map_err(|_| invalid("resolved_path"))?;
    if path != reported_path {
        return Err(invalid("resolved_path_symlink"));
    }
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| invalid("path_outside_install_root"))?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("resolved_path"));
    }

    let file = open_executable_at(&root, &components)?;
    hash_executable(file)
}

fn open_executable_at(root: &Path, components: &[Component<'_>]) -> Result<fs::File, MiseError> {
    let mut directory = open_directory_chain(root)?;
    for component in &components[..components.len() - 1] {
        let Component::Normal(name) = component else {
            return Err(invalid("resolved_path"));
        };
        let child = openat(
            &directory,
            *name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| invalid("resolved_path"))?;
        let stat = rustix::fs::fstat(&child).map_err(|_| invalid("resolved_path"))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
            || stat.st_uid != rustix::process::geteuid().as_raw()
        {
            return Err(invalid("resolved_path"));
        }
        directory = fs::File::from(child);
    }
    let Some(Component::Normal(name)) = components.last() else {
        return Err(invalid("resolved_path"));
    };
    let file = openat(
        &directory,
        *name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| invalid("resolved_path"))?;
    let stat = rustix::fs::fstat(&file).map_err(|_| invalid("resolved_path"))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o111 == 0
    {
        return Err(invalid("resolved_executable_untrusted"));
    }
    Ok(fs::File::from(file))
}

fn hash_executable(file: fs::File) -> Result<String, MiseError> {
    let mut reader = file.take(EXECUTABLE_MAX_BYTES.saturating_add(1));
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| invalid("resolved_executable_unreadable"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| invalid("resolved_executable_limit"))?)
            .ok_or_else(|| invalid("resolved_executable_limit"))?;
        if total > EXECUTABLE_MAX_BYTES {
            return Err(invalid("resolved_executable_oversize"));
        }
        hash.update(&buffer[..count]);
    }
    let digest = hash.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

fn open_directory_chain(path: &Path) -> Result<fs::File, MiseError> {
    let mut current = fs::File::from(
        rustix::fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| invalid("install_root"))?,
    );
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let child = openat(
                    &current,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| invalid("install_root"))?;
                current = fs::File::from(child);
            }
            _ => return Err(invalid("install_root")),
        }
    }
    let stat = rustix::fs::fstat(&current).map_err(|_| invalid("install_root"))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
    {
        return Err(invalid("install_root"));
    }
    Ok(current)
}

fn invalid(field: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: format!("mise_executable_lookup_{field}"),
        value: "invalid".to_owned(),
    }
}

#[cfg(test)]
#[path = "requests_executable_lookup_tests.rs"]
mod tests;
