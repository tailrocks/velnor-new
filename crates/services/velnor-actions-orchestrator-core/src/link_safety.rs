//! Symlink-component rejection along a root-joined relative path.
//!
//! Evidence verification, staged-proof reads, and check preparation
//! share this one walk: every component must be a normal segment and
//! no traversed prefix may be a symlink, including in-repository
//! parent links. Missing trailing components are not links; the
//! bounded read that follows reports them as missing instead.

use std::path::{Component, Path};

use crate::OrchestratorError;
use crate::error::internal;

/// Refuse every symlink component, including in-repository parent links.
pub fn reject_link_components(root: &Path, relative: &str) -> Result<(), OrchestratorError> {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err(internal("check_path_escape"));
        };
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(internal("check_path_symlink"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(internal("check_path_unreadable")),
        }
    }
    Ok(())
}
