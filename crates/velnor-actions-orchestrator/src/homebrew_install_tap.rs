use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use rustix::fs::{self as ufs, AtFlags, Mode, OFlags, RenameFlags, renameat_with, unlinkat};
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
        Ok(Self {
            path,
            formula: formula.to_owned(),
            held,
        })
    }

    pub(super) fn capture_existing(
        formula: &str,
        bytes: &[u8],
        digest: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let path = Path::new(HOMEBREW_CANDIDATE_FORMULAE).join(format!("{formula}.rb"));
        verify_trusted_path(&path, false)?;
        let held = HeldFile::new(&path, digest, bytes.len() as u64, deadline)?;
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
            path,
            formula: formula.to_owned(),
            held,
        })
    }

    pub(super) fn verify(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.held.verify(deadline)
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
        self.verify(deadline)?;
        staged.verify(deadline)?;
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
        renameat_with(
            &directory,
            stage
                .file_name()
                .ok_or_else(|| invalid("tap_formula_stage_name"))?,
            &directory,
            self.path
                .file_name()
                .ok_or_else(|| invalid("tap_formula_name"))?,
            RenameFlags::EXCHANGE,
        )
        .map_err(io_error)?;
        let displaced = ufs::statat(
            &directory,
            stage
                .file_name()
                .ok_or_else(|| invalid("tap_formula_stage_name"))?,
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(io_error)?;
        let held = self.held.file().metadata().map_err(io_error)?;
        if i128::from(displaced.st_dev) != i128::from(held.dev())
            || displaced.st_ino != held.ino()
            || displaced.st_uid != held.uid()
        {
            return Err(invalid("tap_formula_displaced_identity_mismatch"));
        }
        let displaced = read_regular_nofollow(&stage, prior_bytes.len())?;
        if displaced != prior_bytes {
            return Err(invalid("tap_formula_displaced_bytes_mismatch"));
        }
        unlinkat(
            &directory,
            stage
                .file_name()
                .ok_or_else(|| invalid("tap_formula_stage_name"))?,
            AtFlags::empty(),
        )
        .map_err(io_error)?;
        fs::File::from(directory).sync_all().map_err(io_error)?;
        verify_trusted_path(parent, true)?;
        let held = HeldFile::new(&self.path, digest, bytes.len() as u64, deadline)?;
        Ok(Self {
            path: self.path.clone(),
            formula: self.formula.clone(),
            held,
        })
    }
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
