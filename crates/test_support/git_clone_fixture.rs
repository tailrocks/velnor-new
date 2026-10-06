//! Local clone creation has no preexisting checkout authority.

use std::io;
use std::path::Path;
use std::process::Command;

use super::git_fixture;

/// Clone a normal local fixture into an absent child of an existing parent.
pub(crate) fn clone_fixture(source: &Path, destination: &Path) -> io::Result<Command> {
    let source = source.canonicalize()?;
    let metadata = std::fs::symlink_metadata(source.join(".git"))?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err(io::Error::other(
            "clone source must own a fixture Git directory",
        ));
    }
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::other("clone destination must name an absent child"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("clone destination must have a parent"))?
        .canonicalize()?;
    let destination = parent.join(name);
    match std::fs::symlink_metadata(&destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => return Err(io::Error::other("clone destination already exists")),
        Err(error) => return Err(error),
    }
    let mut command = git_fixture::sterile_command();
    command
        .current_dir(parent)
        .args(["clone", "--local", "--no-hardlinks", "--template=", "--"])
        .arg(source)
        .arg(destination);
    Ok(command)
}
