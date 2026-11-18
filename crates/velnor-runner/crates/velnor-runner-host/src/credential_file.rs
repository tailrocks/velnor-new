// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use rustix::fs::{Mode, OFlags};
use zeroize::Zeroizing;

use crate::error::HostError;
use crate::keychain::read_secret;

/// Mode bits a credential file may carry: owner read/write, group read.
const ALLOWED_MODE: u32 = 0o640;
const READ_BITS: u32 = 0o440;
const GROUP_READ: u32 = 0o040;
const ROOT: u32 = 0;

/// Who the process runs as, as far as file access is concerned.
pub(crate) struct Identity {
    pub(crate) euid: u32,
    pub(crate) egid: u32,
    pub(crate) groups: Vec<u32>,
}

impl Identity {
    fn current() -> Self {
        Self {
            euid: rustix::process::geteuid().as_raw(),
            egid: rustix::process::getegid().as_raw(),
            groups: rustix::process::getgroups()
                .unwrap_or_default()
                .into_iter()
                .map(rustix::process::Gid::as_raw)
                .collect(),
        }
    }
}

/// The access rule for a credential file, applied to the opened file's own metadata.
///
/// - No world access, no group write, no exec or setuid/setgid/sticky bits, and at least one
///   read bit set.
/// - Owned by the effective user or by root. Kubernetes secret volume files are owned by root.
/// - If group-readable, the group must be ours: the egid or a supplementary group. Kubernetes
///   gives the pod `fsGroup` as a supplementary group and mounts the files as root:fsGroup with
///   `defaultMode: 0440`.
pub(crate) fn permits(mode: u32, uid: u32, gid: u32, identity: &Identity) -> bool {
    let mode = mode & 0o7777;
    mode & !ALLOWED_MODE == 0
        && mode & READ_BITS != 0
        && (uid == identity.euid || uid == ROOT)
        && (mode & GROUP_READ == 0 || gid == identity.egid || identity.groups.contains(&gid))
}

/// Symlinks are followed on purpose: kubelet lays a Secret volume out as `token -> ..data/token`
/// with `..data` itself a symlink. The checks run on the opened descriptor, so they apply to the
/// final target and cannot be raced by swapping the link after the check.
pub(crate) fn load(path: &Path) -> Result<Zeroizing<Vec<u8>>, HostError> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| HostError::Keychain)?;
    let mut file = File::from(fd);
    let metadata = file.metadata().map_err(|_| HostError::Keychain)?;
    if !metadata.is_file()
        || !permits(
            metadata.permissions().mode(),
            metadata.uid(),
            metadata.gid(),
            &Identity::current(),
        )
    {
        return Err(HostError::Keychain);
    }
    let secret = read_secret(&mut file)?;
    Ok(secret.into_bytes())
}

#[cfg(test)]
mod tests;
