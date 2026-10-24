use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{self as unix_fs, Mode, OFlags};
use sha2::{Digest, Sha256};
use velnor_actions_mise::CheckDeadline;

use super::{
    CAPTURE_LIMIT, ElfFacts, HOMEBREW_EXECUTABLE, HomebrewInstallEvidence, HomebrewInstallRequest,
    KegState, Snapshot,
};
use crate::{
    OrchestratorError,
    homebrew::command::{HomebrewCommand, homebrew_command},
    internal::internal,
};

pub(super) fn run_brew_text(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let output = run_command(operation, home, temp, deadline)?;
    String::from_utf8(output.stdout).map_err(|_| invalid("homebrew_output_not_utf8"))
}

pub(super) fn run_brew(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    run_command(operation, home, temp, deadline).map(|_| ())
}

pub(super) fn run_command(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<velnor_actions_mise::ProcessOutput, OrchestratorError> {
    let output = homebrew_command(operation, home, temp)
        .map_err(|_| invalid("homebrew_command_rejected"))?
        .run_quiescent_until(CAPTURE_LIMIT, deadline)
        .map_err(|_| invalid("homebrew_command_failed"))?;
    let (output, _drained) = output;
    output
        .require_success(HOMEBREW_EXECUTABLE)
        .map_err(|_| invalid("homebrew_command_failed"))?;
    Ok(output)
}

pub(super) fn trusted_owned_directory(path: &Path) -> Result<PathBuf, OrchestratorError> {
    verify_trusted_path(path, true)?;
    let metadata = fs::metadata(path).map_err(io_error)?;
    if metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(invalid("owned_install_directory_untrusted"));
    }
    Ok(path.to_path_buf())
}

pub(super) fn verify_trusted_path(
    path: &Path,
    final_directory: bool,
) -> Result<(), OrchestratorError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("unsafe_absolute_path"));
    }
    let mut current = PathBuf::from("/");
    for component in path.components().skip(1) {
        let Component::Normal(name) = component else {
            return Err(invalid("unsafe_absolute_path"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("symlink_in_trusted_path"));
        }
        let current_is_final = current == path;
        if !current_is_final {
            let trusted_owner =
                metadata.uid() == 0 || metadata.uid() == crate::filesystem_owner::effective_uid();
            let shared_writable = metadata.permissions().mode() & 0o022 != 0;
            let root_sticky = metadata.uid() == 0 && metadata.permissions().mode() & 0o1000 != 0;
            if !metadata.is_dir() || !trusted_owner || shared_writable && !root_sticky {
                return Err(invalid("untrusted_path_ancestor"));
            }
        }
        if current_is_final && final_directory && !metadata.is_dir() {
            return Err(invalid("trusted_path_not_directory"));
        }
    }
    Ok(())
}

pub(super) fn read_regular_nofollow(
    path: &Path,
    limit: usize,
) -> Result<Vec<u8>, OrchestratorError> {
    verify_trusted_path(path, false)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("file_parent_missing"))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("file_name_missing"))?;
    let directory = unix_fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io_error)?;
    let file = unix_fs::openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(io_error)?;
    let file = File::from(file);
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.len()
            > u64::try_from(limit).map_err(|_| invalid("regular_file_limit_overflow"))?
    {
        return Err(invalid("regular_file_limit_or_type"));
    }
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| invalid("regular_file_capacity_overflow"))?;
    let read_limit = u64::try_from(limit)
        .map_err(|_| invalid("regular_file_limit_overflow"))?
        .checked_add(1)
        .ok_or_else(|| invalid("regular_file_limit_overflow"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if u64::try_from(bytes.len()).ok() != Some(metadata.len()) {
        return Err(invalid("regular_file_changed_while_reading"));
    }
    Ok(bytes)
}

pub(super) fn snapshot(path: &Path) -> Result<Snapshot, OrchestratorError> {
    verify_trusted_path(path, true)?;
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(invalid("keg_not_directory"));
    }
    Ok(snapshot_metadata(&metadata))
}

pub(super) fn open_keg_directory(path: &Path) -> Result<File, OrchestratorError> {
    verify_trusted_path(path, true)?;
    let descriptor = unix_fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io_error)?;
    let directory = File::from(descriptor);
    let _ = snapshot_from_file(&directory)?;
    Ok(directory)
}

pub(super) fn snapshot_from_file(file: &File) -> Result<Snapshot, OrchestratorError> {
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_dir()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(invalid("keg_descriptor_not_trusted_directory"));
    }
    Ok(snapshot_metadata(&metadata))
}

pub(super) fn snapshot_metadata(metadata: &fs::Metadata) -> Snapshot {
    Snapshot {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        size: metadata.size(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }
}

pub(super) fn evidence_from_state(
    formula: &str,
    request: &HomebrewInstallRequest<'_>,
    brew_version: &str,
    prefix: &str,
    cellar: &str,
    keg: &Path,
    state: &KegState,
) -> HomebrewInstallEvidence {
    HomebrewInstallEvidence {
        formula: formula.to_owned(),
        version: request.literals.version().to_owned(),
        brew_version: brew_version.trim().to_owned(),
        prefix: prefix.trim().to_owned(),
        cellar: cellar.trim().to_owned(),
        keg: keg.to_string_lossy().into_owned(),
        install_time: state.install_time,
        receipt_sha256: state.receipt_sha256.clone(),
        recipe_sha256: state.recipe_sha256.clone(),
        payload_sha256: state.payload_sha256.clone(),
        installed_files: state.installed_files.clone(),
        version_output_sha256: state.version_output_sha256.clone(),
        verification_sha256: String::new(),
    }
}

pub(super) fn epoch_seconds() -> Result<u64, OrchestratorError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .ok_or_else(|| invalid("system_clock_before_epoch"))
}

/// Read offline ELF identity facts from authenticated archive-member bytes.
///
/// This inspects only the header and never executes or writes the member.
pub(crate) fn elf_facts_from_bytes(bytes: &[u8]) -> Result<ElfFacts, OrchestratorError> {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" {
        return Err(invalid("linux_payload_not_elf"));
    }
    let class = bytes[4];
    let endian = bytes[5];
    if !matches!(class, 1 | 2) || !matches!(endian, 1 | 2) {
        return Err(invalid("linux_payload_invalid_elf_header"));
    }
    let machine = match endian {
        1 => u16::from_le_bytes([bytes[18], bytes[19]]),
        2 => u16::from_be_bytes([bytes[18], bytes[19]]),
        _ => unreachable!(),
    };
    Ok(ElfFacts {
        class,
        endian,
        machine,
    })
}

pub(super) fn elf_facts(bytes: &[u8]) -> Result<ElfFacts, OrchestratorError> {
    elf_facts_from_bytes(bytes)
}

pub(super) fn expected_elf_for_asset(
    request: &HomebrewInstallRequest<'_>,
    asset: &str,
) -> Option<ElfFacts> {
    let payload = request
        .projection
        .selected_payloads()
        .iter()
        .find(|payload| payload.asset_name() == asset)?;
    let (class, endian, machine) = match payload.target().split('-').next()? {
        "aarch64" => (2, 1, 183),
        "x86_64" => (2, 1, 62),
        "i686" => (1, 1, 3),
        "armv7" => (1, 1, 40),
        "riscv64" => (2, 1, 243),
        _ => return None,
    };
    Some(ElfFacts {
        class,
        endian,
        machine,
    })
}

pub(super) fn is_supported_host_macho(bytes: &[u8]) -> bool {
    bytes.len() >= 8
        && bytes[..4] == [0xcf, 0xfa, 0xed, 0xfe]
        && u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) == 0x0100_000c
}

pub(super) fn evidence_digest(evidence: &HomebrewInstallEvidence) -> String {
    let mut digest = Sha256::new();
    for value in [
        evidence.formula.as_bytes(),
        evidence.version.as_bytes(),
        evidence.brew_version.as_bytes(),
        evidence.prefix.as_bytes(),
        evidence.cellar.as_bytes(),
        evidence.keg.as_bytes(),
        evidence.install_time.to_be_bytes().as_slice(),
        evidence.receipt_sha256.as_bytes(),
        evidence.recipe_sha256.as_bytes(),
        evidence.payload_sha256.as_bytes(),
        evidence.version_output_sha256.as_bytes(),
    ] {
        digest_field(&mut digest, value);
    }
    for file in &evidence.installed_files {
        digest_field(&mut digest, file.asset.as_bytes());
        digest_field(&mut digest, file.member.as_bytes());
        digest_field(&mut digest, file.destination.to_string_lossy().as_bytes());
        digest_field(&mut digest, file.sha256.as_bytes());
        digest_field(&mut digest, file.mode.to_be_bytes().as_slice());
        digest_field(&mut digest, format!("{:?}", file.test).as_bytes());
        if let Some(elf) = file.elf {
            digest_field(&mut digest, [elf.class, elf.endian].as_slice());
            digest_field(&mut digest, elf.machine.to_be_bytes().as_slice());
        }
    }
    hex(&digest.finalize())
}

pub(super) fn digest_field(digest: &mut Sha256, field: &[u8]) {
    digest.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(field);
}

pub(super) fn path_exists(path: &Path) -> Result<bool, OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

pub(super) fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn current_host_target() -> Result<&'static str, OrchestratorError> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("aarch64-apple-darwin")
    } else {
        Err(invalid("unsupported_homebrew_host"))
    }
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub(super) fn io_error(_error: impl std::fmt::Display) -> OrchestratorError {
    internal("homebrew_install_io")
}

pub(super) fn invalid(problem: &'static str) -> OrchestratorError {
    internal(&format!("homebrew_install_{problem}"))
}

#[cfg(test)]
#[path = "homebrew_install_tests.rs"]
mod tests;
