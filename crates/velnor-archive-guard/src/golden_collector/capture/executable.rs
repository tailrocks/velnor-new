// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, Metadata, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use velnor_actions_mise::BoundedCommandOutput;

use super::super::CollectorError;
use super::super::binary_format;
use super::super::guard::Guard;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    device: u64,
    inode: u64,
    links: u64,
}

impl From<&Metadata> for Identity {
    fn from(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl From<&Metadata> for DirectoryIdentity {
    fn from(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

pub(super) struct Executable {
    path: PathBuf,
    parent: PathBuf,
    file_identity: Identity,
    parent_identity: DirectoryIdentity,
    owner: u32,
    sha256: String,
    target: &'static str,
}

impl Executable {
    pub(super) fn admit(source: &Path, workspace: &Path) -> Result<Self, CollectorError> {
        let owner = rustix::process::geteuid().as_raw();
        let (input, source_before) = open_source(source)?;
        let executable = Self::copy_into(source, input, &source_before, workspace, owner)?;
        executable.verify()?;
        Ok(executable)
    }

    fn copy_into(
        source: &Path,
        mut input: File,
        source_before: &Metadata,
        workspace: &Path,
        owner: u32,
    ) -> Result<Self, CollectorError> {
        let workspace_before = private_directory(workspace, owner)?;
        let parent = workspace.join("cli");
        fs::create_dir(&parent).map_err(CollectorError::operational)?;
        fs::set_permissions(&parent, Permissions::from_mode(0o700))
            .map_err(CollectorError::operational)?;
        let workspace_metadata = private_directory(workspace, owner)?;
        if DirectoryIdentity::from(&workspace_metadata)
            != DirectoryIdentity::from(&workspace_before)
        {
            return Err(CollectorError::operational(
                "candidate executable workspace changed while preparing its private copy",
            ));
        }
        let parent_metadata = private_directory(&parent, owner)?;
        let path = parent.join("velnor-actions");
        let mut output = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)
            .map_err(CollectorError::operational)?;
        let (copied, sha256) = copy_bytes(&mut input, &mut output)?;
        output.flush().map_err(CollectorError::operational)?;
        output.sync_all().map_err(CollectorError::operational)?;
        verify_source_after_copy(source, source_before, &input)?;
        let target =
            binary_format::target(&mut output, copied).map_err(CollectorError::operational)?;
        output
            .set_permissions(Permissions::from_mode(0o700))
            .map_err(CollectorError::operational)?;
        let file_metadata = output.metadata().map_err(CollectorError::operational)?;
        validate_executable(&file_metadata, owner)?;
        if file_metadata.len() != copied
            || Identity::from(&file_metadata)
                != Identity::from(
                    &fs::symlink_metadata(&path).map_err(CollectorError::operational)?,
                )
        {
            return Err(CollectorError::operational(
                "candidate executable copy changed while being admitted",
            ));
        }
        drop(output);
        if DirectoryIdentity::from(&workspace_metadata)
            != DirectoryIdentity::from(&private_directory(workspace, owner)?)
            || DirectoryIdentity::from(&parent_metadata)
                != DirectoryIdentity::from(&private_directory(&parent, owner)?)
        {
            return Err(CollectorError::operational(
                "candidate executable workspace changed while being admitted",
            ));
        }
        Ok(Self {
            path,
            parent,
            file_identity: Identity::from(&file_metadata),
            parent_identity: DirectoryIdentity::from(&parent_metadata),
            owner,
            sha256,
            target,
        })
    }

    pub(super) fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(super) fn target(&self) -> &'static str {
        self.target
    }

    pub(super) fn command(&self) -> Command {
        Command::new(&self.path)
    }

    pub(super) fn run_candidate(
        &self,
        guard: &Guard,
        root: &Path,
        command: &mut Command,
        persist: impl FnOnce(&BoundedCommandOutput) -> Result<(), CollectorError>,
    ) -> Result<BoundedCommandOutput, CollectorError> {
        self.verify()?;
        let command_result = guard.run_candidate(root, command, persist);
        let executable_result = self.verify();
        match (command_result, executable_result) {
            (Ok(output), Ok(())) => Ok(output),
            (Err(command_error), Ok(())) => Err(command_error),
            (Ok(_), Err(executable_error)) => Err(executable_error),
            (Err(command_error), Err(executable_error)) => Err(CollectorError::operational(
                format!("{command_error}; {executable_error}"),
            )),
        }
    }

    fn verify(&self) -> Result<(), CollectorError> {
        let result = self.verify_identity();
        result.map_err(|error| {
            CollectorError::operational(format!("candidate executable identity changed: {error}"))
        })
    }

    fn verify_identity(&self) -> Result<(), CollectorError> {
        let parent_metadata = private_directory(&self.parent, self.owner)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(CollectorError::operational)?;
        validate_executable(&metadata, self.owner)?;
        if fs::canonicalize(&self.path).map_err(CollectorError::operational)? != self.path
            || DirectoryIdentity::from(&parent_metadata) != self.parent_identity
            || Identity::from(&metadata) != self.file_identity
            || digest_file(&self.path, self.file_identity, self.owner)? != self.sha256
        {
            return Err(CollectorError::operational("identity or digest mismatch"));
        }
        let final_metadata =
            fs::symlink_metadata(&self.path).map_err(CollectorError::operational)?;
        let final_parent = private_directory(&self.parent, self.owner)?;
        if Identity::from(&final_metadata) != self.file_identity
            || DirectoryIdentity::from(&final_parent) != self.parent_identity
        {
            return Err(CollectorError::operational(
                "path changed while checking identity",
            ));
        }
        Ok(())
    }
}

fn open_source(source: &Path) -> Result<(File, Metadata), CollectorError> {
    let source_before = fs::symlink_metadata(source).map_err(CollectorError::operational)?;
    validate_source(&source_before)?;
    if !source.is_absolute()
        || fs::canonicalize(source).map_err(CollectorError::operational)? != source
    {
        return Err(CollectorError::operational(
            "candidate executable path is not canonical",
        ));
    }
    let fd = rustix::fs::open(
        source,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(CollectorError::operational)?;
    let input = File::from(fd);
    let input_metadata = input.metadata().map_err(CollectorError::operational)?;
    validate_source(&input_metadata)?;
    if Identity::from(&source_before) != Identity::from(&input_metadata) {
        return Err(CollectorError::operational(
            "candidate executable changed while opening",
        ));
    }
    Ok((input, source_before))
}

fn copy_bytes(input: &mut File, output: &mut File) -> Result<(u64, String), CollectorError> {
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(CollectorError::operational)?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(CollectorError::operational)?;
        hasher.update(&buffer[..count]);
        copied = copied
            .checked_add(count as u64)
            .ok_or_else(|| CollectorError::operational("candidate executable is too large"))?;
    }
    Ok((copied, hex(&hasher.finalize())))
}

fn verify_source_after_copy(
    source: &Path,
    admitted: &Metadata,
    input: &File,
) -> Result<(), CollectorError> {
    let opened = input.metadata().map_err(CollectorError::operational)?;
    let current = fs::symlink_metadata(source).map_err(CollectorError::operational)?;
    validate_source(&opened)?;
    validate_source(&current)?;
    if Identity::from(admitted) != Identity::from(&opened)
        || Identity::from(admitted) != Identity::from(&current)
        || fs::canonicalize(source).map_err(CollectorError::operational)? != source
    {
        return Err(CollectorError::operational(
            "candidate executable path changed while being copied",
        ));
    }
    Ok(())
}

fn private_directory(path: &Path, owner: u32) -> Result<Metadata, CollectorError> {
    let metadata = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    if !metadata.file_type().is_dir()
        || fs::canonicalize(path).map_err(CollectorError::operational)? != path
        || metadata.uid() != owner
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CollectorError::operational(
            "candidate executable directory must be private to the current user",
        ));
    }
    Ok(metadata)
}

fn validate_executable(metadata: &Metadata, owner: u32) -> Result<(), CollectorError> {
    let mode = metadata.permissions().mode();
    if !metadata.file_type().is_file()
        || metadata.uid() != owner
        || metadata.nlink() != 1
        || mode & 0o022 != 0
        || mode & 0o111 == 0
    {
        return Err(CollectorError::operational(
            "candidate executable ownership, permissions, or link count are unsafe",
        ));
    }
    Ok(())
}

fn validate_source(metadata: &Metadata) -> Result<(), CollectorError> {
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(CollectorError::operational(
            "candidate executable must be a regular executable file",
        ));
    }
    Ok(())
}

fn digest_file(path: &Path, expected: Identity, owner: u32) -> Result<String, CollectorError> {
    let initial = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    validate_executable(&initial, owner)?;
    if Identity::from(&initial) != expected {
        return Err(CollectorError::operational(
            "candidate file identity mismatch",
        ));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(CollectorError::operational)?;
    let mut file = File::from(fd);
    let opened = file.metadata().map_err(CollectorError::operational)?;
    validate_executable(&opened, owner)?;
    if Identity::from(&opened) != expected {
        return Err(CollectorError::operational(
            "candidate file changed while opening",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(CollectorError::operational)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let final_file = file.metadata().map_err(CollectorError::operational)?;
    let final_path = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    validate_executable(&final_file, owner)?;
    validate_executable(&final_path, owner)?;
    if Identity::from(&final_file) != expected
        || Identity::from(&final_path) != expected
        || fs::canonicalize(path).map_err(CollectorError::operational)? != path
    {
        return Err(CollectorError::operational(
            "candidate file changed while hashing",
        ));
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
