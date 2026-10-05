use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

#[path = "document_shared_scripts_execution_outcomes.rs"]
mod outcomes;
pub(super) use outcomes::{CasePaths, Outcome, case_paths, collect_outcome};

pub(super) struct OwnedTempDir {
    path: Option<PathBuf>,
    parent: PathBuf,
    canonical_parent: PathBuf,
    canonical_path: PathBuf,
    parent_identity: DirectoryIdentity,
    child_identity: DirectoryIdentity,
    cleanup_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
}

impl DirectoryIdentity {
    fn capture(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "execution fixture path is not a real directory",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            mode: metadata.permissions().mode() & 0o7777,
        })
    }

    fn same_object(self, other: Self) -> bool {
        self.device == other.device && self.inode == other.inode && self.uid == other.uid
    }
}

impl OwnedTempDir {
    pub(super) fn path(&self) -> &Path {
        self.path.as_deref().expect("owned temp path")
    }

    pub(super) fn cleanup(&mut self) -> io::Result<()> {
        if !self.cleanup_safe {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove execution fixture before process-group quiescence",
            ));
        }
        let path = self.path();
        verify_owned_directory(self, path)?;
        fs::remove_dir_all(path)?;
        self.path = None;
        Ok(())
    }

    pub(super) fn mark_process_group_pending(&mut self) {
        self.cleanup_safe = false;
    }

    pub(super) fn mark_process_group_quiescent(&mut self) {
        self.cleanup_safe = true;
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        if !self.cleanup_safe {
            eprintln!(
                "shared-script execution fixture retained without proven process-group quiescence: {}",
                path.display()
            );
            return;
        }
        if let Err(error) = (|| {
            verify_owned_directory(self, path)?;
            fs::remove_dir_all(path)
        })() {
            eprintln!("shared-script execution fixture cleanup failed: {error}");
        }
    }
}

fn verify_owned_directory(owner: &OwnedTempDir, path: &Path) -> io::Result<()> {
    if fs::canonicalize(&owner.parent)? != owner.canonical_parent
        || DirectoryIdentity::capture(&owner.parent)? != owner.parent_identity
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "temporary execution parent identity changed",
        ));
    }
    if fs::canonicalize(path)? != owner.canonical_path
        || DirectoryIdentity::capture(path)? != owner.child_identity
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "temporary execution fixture identity changed",
        ));
    }
    Ok(())
}

pub(super) fn new_root() -> io::Result<OwnedTempDir> {
    let parent = fs::canonicalize(std::env::temp_dir())?;
    let parent_identity = DirectoryIdentity::capture(&parent)?;
    if parent_identity.mode & 0o022 != 0 && parent_identity.mode & 0o1000 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "writable temporary parent is not sticky",
        ));
    }
    let uid = effective_uid()?;
    for _ in 0..128 {
        let id = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            "velnor-shared-script-execution-{}-{id}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => return finish_root_setup(path, &parent, parent_identity, uid),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique execution fixture",
    ))
}

fn finish_root_setup(
    path: PathBuf,
    parent: &Path,
    parent_identity: DirectoryIdentity,
    uid: u32,
) -> io::Result<OwnedTempDir> {
    let initial = DirectoryIdentity::capture(&path)?;
    if initial.uid != uid || initial.mode & 0o077 != 0 {
        return rollback_root_setup(
            &path,
            parent,
            parent_identity,
            initial,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "execution fixture ownership or mode is unsafe",
            ),
        );
    }
    let setup = (|| {
        let directory = fs::File::open(&path)?;
        let opened = directory.metadata()?;
        let opened_identity = DirectoryIdentity {
            device: opened.dev(),
            inode: opened.ino(),
            uid: opened.uid(),
            mode: opened.permissions().mode() & 0o7777,
        };
        if !opened.is_dir() || !opened_identity.same_object(initial) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "execution fixture changed before permission setup",
            ));
        }
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        let child_identity = DirectoryIdentity::capture(&path)?;
        if !child_identity.same_object(initial) || child_identity.mode != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "execution fixture changed during permission setup",
            ));
        }
        let canonical_path = fs::canonicalize(&path)?;
        let canonical_parent = fs::canonicalize(parent)?;
        if canonical_parent != parent || DirectoryIdentity::capture(parent)? != parent_identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "temporary execution parent changed during setup",
            ));
        }
        Ok((canonical_parent, canonical_path, child_identity))
    })();
    match setup {
        Ok((canonical_parent, canonical_path, child_identity)) => Ok(OwnedTempDir {
            path: Some(path),
            parent: parent.to_path_buf(),
            canonical_parent,
            canonical_path,
            parent_identity,
            child_identity,
            cleanup_safe: true,
        }),
        Err(setup_error) => {
            rollback_root_setup(&path, parent, parent_identity, initial, setup_error)
        }
    }
}

fn rollback_root_setup(
    path: &Path,
    parent: &Path,
    parent_identity: DirectoryIdentity,
    initial: DirectoryIdentity,
    setup_error: io::Error,
) -> io::Result<OwnedTempDir> {
    let cleanup = (|| {
        if DirectoryIdentity::capture(parent)? != parent_identity
            || !DirectoryIdentity::capture(path)?.same_object(initial)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to remove changed execution fixture during rollback",
            ));
        }
        fs::remove_dir(path)
    })();
    match cleanup {
        Ok(()) => Err(setup_error),
        Err(cleanup_error) => Err(io::Error::new(
            setup_error.kind(),
            format!("fixture setup failed ({setup_error}); rollback failed ({cleanup_error})"),
        )),
    }
}

fn effective_uid() -> io::Result<u32> {
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "could not determine execution fixture uid",
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
        .trim()
        .parse()
        .map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid uid: {error}"))
        })
}

#[cfg(test)]
#[path = "document_shared_scripts_execution_support_tests.rs"]
mod tests;
