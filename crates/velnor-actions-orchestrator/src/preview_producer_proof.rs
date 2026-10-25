use super::checkpoint;
use crate::{OrchestratorError, internal::internal};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use velnor_actions_mise::CheckDeadline;

#[derive(Default)]
pub(super) struct ProofGuard {
    created: Option<(PathBuf, fs::File)>,
}

impl ProofGuard {
    fn cleanup(&mut self) -> Result<(), OrchestratorError> {
        let Some((path, file)) = self.created.take() else {
            return Ok(());
        };
        let owned = file
            .metadata()
            .map_err(|_| internal("preview_proof_identity"))?;
        match fs::symlink_metadata(&path) {
            Ok(current)
                if current.is_file()
                    && current.dev() == owned.dev()
                    && current.ino() == owned.ino() =>
            {
                fs::remove_file(path).map_err(|_| internal("preview_proof_cleanup"))?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(internal("preview_proof_identity")),
        }
        Ok(())
    }
}

impl Drop for ProofGuard {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            eprintln!("preview_proof_cleanup_failed");
        }
    }
}

pub(super) fn with_proof_ownership(
    operation: impl FnOnce(&mut ProofGuard) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let mut guard = ProofGuard::default();
    if let Err(error) = operation(&mut guard) {
        guard.cleanup()?;
        return Err(error);
    }
    guard.created = None;
    Ok(())
}

pub(super) fn publish_proof(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
    guard: &mut ProofGuard,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    let pending = path.with_extension("pending");
    super::handoff::write_upload(&pending, bytes, deadline)?;
    let file = fs::File::from(
        rustix::fs::open(
            &pending,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("preview_proof_identity"))?,
    );
    if !file
        .metadata()
        .map_err(|_| internal("preview_proof_identity"))?
        .is_file()
    {
        return Err(internal("preview_proof_identity"));
    }
    checkpoint(deadline)?;
    if fs::hard_link(&pending, path).is_err() {
        fs::remove_file(&pending).map_err(|_| internal("preview_proof_cleanup"))?;
        return Err(internal("preview_proof_publish"));
    }
    guard.created = Some((path.to_owned(), file));
    fs::remove_file(&pending).map_err(|_| internal("preview_proof_cleanup"))?;
    checkpoint(deadline)
}
