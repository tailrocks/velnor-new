//! A complete local byte closure sealed before cryptographic admission.
use crate::{
    OrchestratorError,
    filesystem_owner::held_file::{HeldFile, checkpoint, same_metadata},
    internal::internal,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug)]
pub(crate) struct HeldDirectory {
    path: PathBuf,
    directory: fs::File,
    original: fs::Metadata,
    files: BTreeMap<String, HeldFile>,
    deadline: CheckDeadline,
    // Last so file and directory handles drop before TempDir removes the tree;
    // shared custody keeps the owner alive through its final Arc.
    _owner: Option<tempfile::TempDir>,
}

impl HeldDirectory {
    pub(crate) fn admit<T>(
        path: &Path,
        deadline: CheckDeadline,
        verify: impl FnOnce() -> Result<T, OrchestratorError>,
    ) -> Result<(T, Self), OrchestratorError> {
        let closure = Self::capture(path, deadline)?;
        let verified = verify()?;
        closure.verify()?;
        Ok((verified, closure))
    }

    pub(crate) fn admit_owned<T>(
        owner: tempfile::TempDir,
        deadline: CheckDeadline,
        verify: impl FnOnce() -> Result<T, OrchestratorError>,
    ) -> Result<(T, Self), OrchestratorError> {
        let closure = Self::capture_owned(owner, deadline)?;
        let verified = verify()?;
        closure.verify()?;
        Ok((verified, closure))
    }

    pub(crate) fn capture(path: &Path, deadline: CheckDeadline) -> Result<Self, OrchestratorError> {
        Self::capture_inner(path, deadline, None)
    }

    pub(crate) fn capture_owned(
        owner: tempfile::TempDir,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let path = owner.path().to_path_buf();
        Self::capture_inner(&path, deadline, Some(owner))
    }

    fn capture_inner(
        path: &Path,
        deadline: CheckDeadline,
        owner: Option<tempfile::TempDir>,
    ) -> Result<Self, OrchestratorError> {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| invalid("open"))?;
        let directory = fs::File::from(fd);
        let original = directory.metadata().map_err(|_| invalid("stat"))?;
        let mut held = Self {
            path: path.to_owned(),
            directory,
            original,
            files: BTreeMap::new(),
            deadline,
            _owner: owner,
        };
        held.verify_identity()?;
        for name in held.names()? {
            let file = HeldFile::capture(&path.join(&name), deadline)?;
            held.files.insert(name, file);
        }
        held.verify()?;
        Ok(held)
    }

    pub(crate) fn verify(&self) -> Result<(), OrchestratorError> {
        self.verify_identity()?;
        let names = self.names()?;
        if names.len() != self.files.len() || names.iter().ne(self.files.keys()) {
            return Err(invalid("inventory"));
        }
        for file in self.files.values() {
            file.verify(self.deadline)?;
        }
        self.verify_identity()?;
        if self.names()? != names {
            return Err(invalid("inventory"));
        }
        checkpoint(self.deadline)
    }

    pub(crate) fn read_file(
        &self,
        name: &str,
        max_size: u64,
    ) -> Result<Vec<u8>, OrchestratorError> {
        checkpoint(self.deadline)?;
        let file = self
            .files
            .get(name)
            .ok_or_else(|| invalid("file_missing"))?;
        let bytes = file.read_bytes(max_size, self.deadline)?;
        self.verify()?;
        Ok(bytes)
    }

    fn verify_identity(&self) -> Result<(), OrchestratorError> {
        checkpoint(self.deadline)?;
        let held = self.directory.metadata().map_err(|_| invalid("stat"))?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid("stat"))?;
        if !held.is_dir()
            || !named.is_dir()
            || !same_metadata(&self.original, &held)
            || !same_metadata(&self.original, &named)
        {
            return Err(invalid("identity"));
        }
        Ok(())
    }

    fn names(&self) -> Result<Vec<String>, OrchestratorError> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.path).map_err(|_| invalid("read"))? {
            checkpoint(self.deadline)?;
            let entry = entry.map_err(|_| invalid("entry"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid("name"))?;
            if names.len() >= 2050 || !entry.file_type().map_err(|_| invalid("kind"))?.is_file() {
                return Err(invalid("inventory"));
            }
            names.push(name);
        }
        if names.is_empty() {
            return Err(invalid("empty"));
        }
        names.sort();
        Ok(names)
    }
}
fn invalid(reason: &str) -> OrchestratorError {
    internal(&format!("held_directory_{reason}"))
}

#[cfg(test)]
#[path = "held_directory_tests.rs"]
mod tests;
