//! Durable independent snapshots keyed by actual owner bytes.
//! Same-user hostile replacement between validation and path exec remains outside
//! this observation: chmod and a path hash are not an operating-system exec seal.

use super::{SessionShims, ShimLease, mark_shim_directory, shim_file_name};
use crate::session::{RUSTC_SHIM_STEM, RUSTDOC_SHIM_STEM};
use eyre::{Result, bail};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// Installation authority for independently copied durable owner shims.
/// Private construction prevents a path-only observation from minting it.
#[derive(Debug, Clone)]
pub(crate) struct SessionDispatchPin {
    directory: PathBuf,
    owner_sha256: String,
}

impl SessionDispatchPin {
    pub(crate) fn owner_sha256(&self) -> &str {
        &self.owner_sha256
    }

    pub(crate) fn verify_route(&self, path: &Path) -> Result<()> {
        let absolute = std::path::absolute(path)?;
        if !absolute.starts_with(&self.directory) {
            bail!("dispatch snapshot is outside its owning source namespace");
        }
        #[cfg(unix)]
        {
            for parent in absolute.parent().into_iter().flat_map(Path::ancestors) {
                if !parent.starts_with(&self.directory) {
                    break;
                }
                publication::verify_directory(parent, true)?;
            }
            publication::verify(&absolute, &self.owner_sha256, 0o500)?;
        }
        #[cfg(not(unix))]
        bail!("owned private snapshot authority is unsupported on this platform");
        Ok(())
    }
}

#[cfg(unix)]
#[path = "pinned_shims/durable.rs"]
mod durable;
#[cfg(unix)]
use super::snapshot_publication as publication;
#[cfg(unix)]
use publication::{digest, same_file};

#[cfg(unix)]
pub(super) fn install_session(configured: &Path) -> Result<SessionShims> {
    let executable = std::env::current_exe()?.canonicalize()?;
    install_session_from(configured, &executable)
}

#[cfg(unix)]
fn install_session_from(configured: &Path, executable: &Path) -> Result<SessionShims> {
    let (directory, expected) = durable::root(configured, executable)?;
    let owner = directory.join(durable::OWNER_FILE);
    install_expected(executable, &owner, &expected)?;
    let rust = directory.join("rust");
    let native = directory.join("native");
    publication::directory(&rust, true)?;
    publication::directory(&native, true)?;
    mark_shim_directory(&rust);
    mark_shim_directory(&native);
    let rustc = rust.join(shim_file_name(RUSTC_SHIM_STEM));
    let rustdoc = directory.join(shim_file_name(RUSTDOC_SHIM_STEM));
    install_expected(&owner, &rustc, &expected)?;
    install_expected(&owner, &rustdoc, &expected)?;
    let lease = ShimLease::take(&rust)?;
    let dispatch_pin = SessionDispatchPin {
        directory,
        owner_sha256: expected,
    };
    dispatch_pin.verify_route(&rustc)?;
    dispatch_pin.verify_route(&rustdoc)?;
    Ok(SessionShims {
        rustc,
        rustdoc,
        native,
        lease,
        dispatch_pin,
    })
}

#[cfg(not(unix))]
pub(super) fn install_session(_configured: &Path) -> Result<SessionShims> {
    bail!("owned private durable snapshots are unsupported on this platform")
}

/// A lazy durable role always copies the initially captured owner snapshot.
#[cfg(unix)]
pub(super) fn install(source: &Path, destination: &Path) -> Result<()> {
    if let Some((owner, expected)) = durable::fixed_source(destination)? {
        return install_expected(&owner, destination, &expected);
    }
    let metadata = publication::source_metadata(source)?;
    let mut input = publication::open_plain(source)?;
    if !same_file(&metadata, &input.metadata()?) {
        bail!("shim source changed while opening");
    }
    let expected = digest(&mut input)?;
    install_expected(source, destination, &expected)
}

#[cfg(not(unix))]
pub(super) fn install(_source: &Path, _destination: &Path) -> Result<()> {
    bail!("owned private durable snapshots are unsupported on this platform")
}

#[cfg(unix)]
fn install_expected(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    install_expected_with(source, destination, expected, || Ok(()))
}

#[cfg(unix)]
fn install_expected_with(
    source: &Path,
    destination: &Path,
    expected: &str,
    after_first_chunk: impl FnOnce() -> Result<()>,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let destination = publication::canonical_parent(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("shim lacks parent"))?;
    if publication::exists(&destination)? {
        let _lock = publication::lock(parent)?;
        return publication::verify(&destination, expected, 0o500);
    }
    let source_metadata = publication::source_metadata(source)?;
    let mut input = publication::open_plain(source)?;
    if !same_file(&source_metadata, &input.metadata()?) || digest(&mut input)? != expected {
        bail!("shim source differs from its captured identity");
    }
    input.rewind()?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    copy_snapshot(&mut input, &mut staged, expected, after_first_chunk)?;
    input.rewind()?;
    if digest(&mut input)? != expected
        || !same_file(&source_metadata, &input.metadata()?)
        || !same_file(&source_metadata, &publication::source_metadata(source)?)
    {
        bail!("shim source changed before publication");
    }
    let mut current = publication::open_plain(source)?;
    if !same_file(&source_metadata, &current.metadata()?) || digest(&mut current)? != expected {
        bail!("shim source path changed before publication");
    }
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o500))?;
    staged.as_file().sync_all()?;
    staged.as_file_mut().rewind()?;
    if digest(staged.as_file_mut())? != expected {
        bail!("shim snapshot changed before publication");
    }
    publication::publish(staged, &destination, expected, 0o500)
}

#[cfg(unix)]
fn copy_snapshot(
    input: &mut File,
    staged: &mut tempfile::NamedTempFile,
    expected: &str,
    after_first_chunk: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut hasher = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut after_first_chunk = Some(after_first_chunk);
    loop {
        let count = input.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        staged.write_all(&bytes[..count])?;
        hasher.update(&bytes[..count]);
        if let Some(hook) = after_first_chunk.take() {
            hook()?;
        }
    }
    if hex::encode(hasher.finalize()) != expected {
        bail!("shim source changed while copying");
    }
    Ok(())
}

#[cfg(test)]
#[path = "pinned_shims_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "pinned_shims/durable_tests.rs"]
mod durable_tests;
