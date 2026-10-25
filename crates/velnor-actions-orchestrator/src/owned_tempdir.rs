use std::{io, os::unix::fs::PermissionsExt, path::Path};

pub(crate) fn private_tempdir_in(parent: &Path, prefix: &str) -> io::Result<tempfile::TempDir> {
    let physical_parent = parent.canonicalize()?;
    let permissions = std::fs::Permissions::from_mode(0o700);
    tempfile::Builder::new()
        .prefix(prefix)
        .permissions(permissions)
        .tempdir_in(physical_parent)
}
