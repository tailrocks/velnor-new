// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use tempfile::{Builder, TempDir};

pub(super) struct Harness {
    pub(super) temp: TempDir,
    pub(super) directory: PathBuf,
    pub(super) root: PathBuf,
    pub(super) collector: PathBuf,
    pub(super) archive_guard: PathBuf,
    pub(super) guard_sha256: String,
    pub(super) guard_source: PathBuf,
}

impl Harness {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let temp_parent = fs::canonicalize("/tmp")?;
        let temp = Builder::new()
            .prefix("golden collector [paths]*.")
            .tempdir_in(temp_parent)?;
        let directory = fs::canonicalize(temp.path())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or("archive guard crate has no repository root")?
            .to_path_buf();
        let root = fs::canonicalize(root)?;
        let collector = PathBuf::from(env!("CARGO_BIN_EXE_velnor-golden-collector"));
        let guard_source = PathBuf::from(env!("CARGO_BIN_EXE_velnor-archive-guard"));
        let guard_directory = directory.join("guard");
        fs::create_dir(&guard_directory)?;
        fs::set_permissions(&guard_directory, fs::Permissions::from_mode(0o700))?;
        let archive_guard = guard_directory.join("archive-guard");
        copy_executable(&guard_source, &archive_guard)?;
        let guard_sha256 = sha256(&fs::read(&archive_guard)?);
        Ok(Self {
            temp,
            directory,
            root,
            collector,
            archive_guard,
            guard_sha256,
            guard_source,
        })
    }

    pub(super) fn copy_candidate(
        &self,
        name: &str,
        change: impl FnOnce(&mut Vec<u8>),
    ) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.directory.join(name);
        let mut bytes = fs::read(&self.guard_source)?;
        change(&mut bytes);
        fs::write(&path, bytes)?;
        set_executable(&path)?;
        Ok(path)
    }

    pub(super) fn run(
        &self,
        mode: &str,
        root: &Path,
        candidate: &Path,
        guard: &Path,
        guard_sha256: &str,
        golden_dir: Option<&Path>,
    ) -> Result<Output, Box<dyn Error>> {
        Ok(self
            .command(mode, root, candidate, guard, guard_sha256, golden_dir)
            .output()?)
    }

    pub(super) fn command(
        &self,
        mode: &str,
        root: &Path,
        candidate: &Path,
        guard: &Path,
        guard_sha256: &str,
        golden_dir: Option<&Path>,
    ) -> Command {
        let mut command = Command::new(&self.collector);
        command
            .arg(mode)
            .arg("--root")
            .arg(root)
            .arg("--cli")
            .arg(candidate)
            .arg("--archive-guard")
            .arg(guard)
            .arg("--archive-guard-sha256")
            .arg(guard_sha256)
            .env("TMPDIR", &self.directory)
            .env_remove("VELNOR_GOLDEN_DIR")
            .env_remove("VELNOR_GOLDEN_KEEP_WORK")
            .env_remove("VELNOR_PLAN_DIAGNOSTICS_MODE")
            .env_remove("VELNOR_PLAN_DIAGNOSTICS_ROOT")
            .env_remove("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT");
        if let Some(golden_dir) = golden_dir {
            command.env("VELNOR_GOLDEN_DIR", golden_dir);
        }
        command
    }
}

pub(super) fn diagnostics_candidate() -> Result<PathBuf, Box<dyn Error>> {
    let path = env::var_os("VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE")
        .ok_or("qualification diagnostics fixture locator is missing")?;
    let path = PathBuf::from(path);
    let metadata = fs::symlink_metadata(&path)?;
    if !path.is_absolute()
        || !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o111 == 0
        || fs::canonicalize(&path)? != path
    {
        return Err("qualification diagnostics fixture is not an absolute executable file".into());
    }
    Ok(path)
}

pub(super) fn copy_fingerprint_inputs(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir(destination)?;
    let manifest = fs::read_to_string(source.join("scripts/archive-guard-inputs.txt"))?;
    copy_file(
        &source.join("scripts/archive-guard-inputs.txt"),
        &destination.join("scripts/archive-guard-inputs.txt"),
    )?;
    for line in manifest.lines() {
        let (kind, relative) = line.split_once(' ').ok_or("invalid input manifest")?;
        let from = source.join(relative);
        let to = destination.join(relative);
        match kind {
            "file" => copy_file(&from, &to)?,
            "optional" if from.exists() => copy_file(&from, &to)?,
            "optional" => {}
            "tree" => copy_tree(&from, &to)?,
            _ => return Err("unknown source fingerprint input kind".into()),
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let parent = destination
        .parent()
        .ok_or("fingerprint input has no parent")?;
    fs::create_dir_all(parent)?;
    fs::copy(source, destination)?;
    Ok(())
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.file_type().is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.file_type().is_file() {
            fs::copy(from, to)?;
        } else if metadata.file_type().is_symlink() {
            symlink(fs::read_link(from)?, to)?;
        } else {
            return Err("source fingerprint tree contains a special file".into());
        }
    }
    Ok(())
}

pub(super) fn copy_executable(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::copy(source, destination)?;
    set_executable(destination)
}

pub(super) fn set_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
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

pub(super) fn retained_workspace(output: &Output) -> Option<PathBuf> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("retaining golden evidence workspace: "))
        .map(PathBuf::from)
}
