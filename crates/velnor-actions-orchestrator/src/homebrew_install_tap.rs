use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use rustix::fs::{
    self as ufs, AtFlags, Dir, FileType, Mode, OFlags, RenameFlags, renameat_with, unlinkat,
};
use velnor_actions_mise::CheckDeadline;

use super::{HOMEBREW_CANDIDATE_FORMULAE, HOMEBREW_CANDIDATE_TAP};
use super::{
    request::write_formula_file,
    support::{
        invalid, io_error, read_regular_nofollow, trusted_owned_directory, verify_trusted_path,
    },
};
use crate::{OrchestratorError, filesystem_owner::held_file::HeldFile};

/// Descriptor-backed identity of the stable formula Homebrew reads and records.
pub(super) struct TapFormula {
    pub(super) path: PathBuf,
    pub(super) formula: String,
    held: HeldFile,
}

impl TapFormula {
    pub(super) fn create_fresh(
        formula: &str,
        bytes: &[u8],
        digest: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        Self::create_fresh_at(
            Path::new(HOMEBREW_CANDIDATE_FORMULAE),
            formula,
            bytes,
            digest,
            deadline,
        )
    }

    fn create_fresh_at(
        directory: &Path,
        formula: &str,
        bytes: &[u8],
        digest: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        ensure_formula_directory(directory)?;
        let path = directory.join(format!("{formula}.rb"));
        write_formula_file(&path, bytes, digest, deadline)?;
        let held = HeldFile::new(&path, digest, bytes.len() as u64, deadline)?;
        let candidate = Self {
            path,
            formula: formula.to_owned(),
            held,
        };
        candidate.validate_inventory(None)?;
        Ok(candidate)
    }

    pub(super) fn capture_existing(
        formula: &str,
        bytes: &[u8],
        digest: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let path = Path::new(HOMEBREW_CANDIDATE_FORMULAE).join(format!("{formula}.rb"));
        Self::capture_existing_at(&path, formula, bytes, digest, deadline)
    }

    fn capture_existing_at(
        path: &Path,
        formula: &str,
        bytes: &[u8],
        digest: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        verify_trusted_path(path, false)?;
        validate_inventory(path, formula, None)?;
        let held = HeldFile::new(path, digest, bytes.len() as u64, deadline)?;
        let metadata = held.file().metadata().map_err(io_error)?;
        if metadata.uid() != crate::filesystem_owner::effective_uid()
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid("tap_formula_owner"));
        }
        if held.read_bytes(bytes.len() as u64, deadline)? != bytes {
            return Err(invalid("tap_formula_prior_bytes_mismatch"));
        }
        Ok(Self {
            path: path.to_path_buf(),
            formula: formula.to_owned(),
            held,
        })
    }

    pub(super) fn verify(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.validate_inventory(None)?;
        self.held.verify(deadline)
    }

    fn validate_inventory(&self, stage_name: Option<&str>) -> Result<(), OrchestratorError> {
        validate_inventory(&self.path, &self.formula, stage_name)
    }

    /// Replace only a verified prior candidate, atomically exchanging in place.
    pub(super) fn replace(
        &self,
        bytes: &[u8],
        digest: &str,
        prior_bytes: &[u8],
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        self.verify(deadline)?;
        if self.held.read_bytes(prior_bytes.len() as u64, deadline)? != prior_bytes {
            return Err(invalid("tap_formula_prior_bytes_mismatch"));
        }
        let stage = self
            .path
            .with_file_name(format!(".velnor-{}.stage", self.formula));
        write_formula_file(&stage, bytes, digest, deadline)?;
        let staged = HeldFile::new(&stage, digest, bytes.len() as u64, deadline)?;
        self.held.verify(deadline)?;
        staged.verify(deadline)?;
        self.exchange_staged_formula(&stage, &staged, bytes, digest, prior_bytes, deadline)
    }

    fn exchange_staged_formula(
        &self,
        stage: &Path,
        staged: &HeldFile,
        bytes: &[u8],
        digest: &str,
        prior_bytes: &[u8],
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let stage_name = stage
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("tap_formula_stage_name"))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid("tap_formula_parent"))?;
        verify_trusted_path(parent, true)?;
        let directory = ufs::open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io_error)?;
        let descriptor_metadata = ufs::fstat(&directory).map_err(io_error)?;
        let named_metadata = fs::symlink_metadata(parent).map_err(io_error)?;
        if !named_metadata.is_dir()
            || named_metadata.file_type().is_symlink()
            || i128::from(descriptor_metadata.st_dev) != i128::from(named_metadata.dev())
            || descriptor_metadata.st_ino != named_metadata.ino()
            || descriptor_metadata.st_uid != crate::filesystem_owner::effective_uid()
        {
            return Err(invalid("tap_formula_parent_identity"));
        }
        self.validate_inventory(Some(stage_name))?;
        self.held.verify(deadline)?;
        staged.verify(deadline)?;
        renameat_with(
            &directory,
            stage_name,
            &directory,
            self.path
                .file_name()
                .ok_or_else(|| invalid("tap_formula_name"))?,
            RenameFlags::EXCHANGE,
        )
        .map_err(io_error)?;
        let displaced =
            ufs::statat(&directory, stage_name, AtFlags::SYMLINK_NOFOLLOW).map_err(io_error)?;
        let held = self.held.file().metadata().map_err(io_error)?;
        if i128::from(displaced.st_dev) != i128::from(held.dev())
            || displaced.st_ino != held.ino()
            || displaced.st_uid != held.uid()
        {
            return Err(invalid("tap_formula_displaced_identity_mismatch"));
        }
        self.validate_inventory(Some(stage_name))?;
        let displaced = read_regular_nofollow(stage, prior_bytes.len())?;
        if displaced != prior_bytes {
            return Err(invalid("tap_formula_displaced_bytes_mismatch"));
        }
        unlinkat(&directory, stage_name, AtFlags::empty()).map_err(io_error)?;
        fs::File::from(directory).sync_all().map_err(io_error)?;
        verify_trusted_path(parent, true)?;
        self.validate_inventory(None)?;
        let held = HeldFile::new(&self.path, digest, bytes.len() as u64, deadline)?;
        held.verify(deadline)?;
        Ok(Self {
            path: self.path.clone(),
            formula: self.formula.clone(),
            held,
        })
    }
}

fn validate_inventory(
    formula_path: &Path,
    formula: &str,
    stage_name: Option<&str>,
) -> Result<(), OrchestratorError> {
    let formula_directory = formula_path
        .parent()
        .ok_or_else(|| invalid("tap_formula_parent"))?;
    let tap_root = formula_directory
        .parent()
        .ok_or_else(|| invalid("tap_root_parent"))?;
    let expected_name = format!("{formula}.rb");
    if formula_path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err(invalid("tap_formula_path_mismatch"));
    }
    validate_directory_entries(tap_root, "Formula", None, true)?;
    validate_directory_entries(formula_directory, &expected_name, stage_name, false)
}

fn validate_directory_entries(
    path: &Path,
    expected_name: &str,
    stage_name: Option<&str>,
    expected_is_directory: bool,
) -> Result<(), OrchestratorError> {
    trusted_owned_directory(path)?;
    let named = fs::symlink_metadata(path).map_err(io_error)?;
    if named.uid() != crate::filesystem_owner::effective_uid() || named.mode() & 0o077 != 0 {
        return Err(invalid("tap_directory_privacy"));
    }
    let directory = ufs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io_error)?;
    let descriptor = ufs::fstat(&directory).map_err(io_error)?;
    if i128::from(descriptor.st_dev) != i128::from(named.dev())
        || descriptor.st_ino != named.ino()
        || descriptor.st_uid != crate::filesystem_owner::effective_uid()
    {
        return Err(invalid("tap_directory_identity"));
    }
    let mut expected_seen = false;
    let mut stage_seen = stage_name.is_none();
    let mut count = 0_u8;
    for entry in Dir::read_from(&directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| invalid("tap_entry_name_not_utf8"))?;
        if name == "." || name == ".." {
            continue;
        }
        count = count.saturating_add(1);
        if count > 2 {
            return Err(invalid("tap_unexpected_entry"));
        }
        let is_expected = name == expected_name && !expected_seen;
        let is_stage = stage_name == Some(name) && !stage_seen;
        if !is_expected && !is_stage {
            return Err(invalid("tap_unexpected_entry"));
        }
        if is_expected {
            expected_seen = true;
        }
        if is_stage {
            stage_seen = true;
        }
        let stat = ufs::statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io_error)?;
        let expected_type = if expected_is_directory && is_expected {
            FileType::Directory
        } else {
            FileType::RegularFile
        };
        if FileType::from_raw_mode(stat.st_mode) != expected_type
            || stat.st_uid != crate::filesystem_owner::effective_uid()
            || !expected_is_directory && (stat.st_nlink != 1 || stat.st_mode & 0o777 != 0o644)
            || expected_is_directory && stat.st_mode & 0o077 != 0
        {
            return Err(invalid("tap_entry_metadata"));
        }
    }
    if !expected_seen || !stage_seen {
        return Err(invalid("tap_expected_entry_missing"));
    }
    let current = fs::symlink_metadata(path).map_err(io_error)?;
    let current_descriptor = ufs::fstat(&directory).map_err(io_error)?;
    if current.file_type().is_symlink()
        || !crate::filesystem_owner::held_file::same_metadata(&named, &current)
        || current_descriptor.st_ino != descriptor.st_ino
        || i128::from(current_descriptor.st_dev) != i128::from(descriptor.st_dev)
    {
        return Err(invalid("tap_directory_changed_during_inventory"));
    }
    Ok(())
}

pub(super) fn formula_reference(formula: &str) -> Result<String, OrchestratorError> {
    validate_formula(formula)?;
    Ok(format!("{HOMEBREW_CANDIDATE_TAP}/{formula}"))
}

fn validate_formula(formula: &str) -> Result<(), OrchestratorError> {
    if !crate::homebrew::command::valid_candidate_formula_name(formula) {
        return Err(invalid("tap_formula_name"));
    }
    Ok(())
}

fn ensure_formula_directory(directory: &Path) -> Result<(), OrchestratorError> {
    let formula_dir = directory;
    let tap_root = formula_dir
        .parent()
        .ok_or_else(|| invalid("tap_formula_root"))?;
    let namespace = tap_root.parent().ok_or_else(|| invalid("tap_parent"))?;
    let taps = namespace.parent().ok_or_else(|| invalid("tap_parent"))?;
    let library = taps.parent().ok_or_else(|| invalid("tap_parent"))?;
    trusted_owned_directory(library)?;
    ensure_directory(taps)?;
    ensure_directory(namespace)?;
    create_owned_directory(tap_root)?;
    create_owned_directory(formula_dir)
}

fn ensure_directory(path: &Path) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(_) => trusted_owned_directory(path).map(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_owned_directory(path),
        Err(error) => Err(io_error(error)),
    }
}

fn create_owned_directory(path: &Path) -> Result<(), OrchestratorError> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(invalid("tap_already_exists"));
        }
        Err(error) => return Err(io_error(error)),
    }
    verify_trusted_path(path, true)?;
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.uid() != crate::filesystem_owner::effective_uid() || metadata.mode() & 0o077 != 0 {
        return Err(invalid("tap_directory_owner"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "homebrew_install_tap_tests.rs"]
mod tests;
