// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use rustix::fs::{Mode, OFlags};
use zeroize::Zeroizing;

use crate::error::HostError;
use crate::keychain::read_secret;
const ALLOWED_MODE: u32 = 0o640;
const READ_BITS: u32 = 0o440;
const GROUP_READ: u32 = 0o040;
const ROOT: u32 = 0;
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
pub(crate) fn permits(mode: u32, uid: u32, gid: u32, identity: &Identity) -> bool {
    let mode = mode & 0o7777;
    mode & !ALLOWED_MODE == 0
        && mode & READ_BITS != 0
        && (uid == identity.euid || uid == ROOT)
        && (mode & GROUP_READ == 0 || gid == identity.egid || identity.groups.contains(&gid))
}
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
    if has_extended_acl(&file) {
        return Err(HostError::Keychain);
    }
    let secret = read_secret(&mut file)?;
    Ok(secret.into_bytes())
}

#[cfg(target_os = "linux")]
fn has_extended_acl(file: &File) -> bool {
    const ACCESS_ACL: &str = "system.posix_acl_access";
    const NFS4_ACL: &str = "system.nfs4_acl";
    [ACCESS_ACL, NFS4_ACL].into_iter().any(|name| {
        !matches!(
            rustix::fs::fgetxattr(file, name, &mut []),
            Err(rustix::io::Errno::NODATA | rustix::io::Errno::NOTSUP)
        )
    })
}

#[cfg(not(target_os = "linux"))]
const fn has_extended_acl(_file: &File) -> bool {
    false
}

#[cfg(test)]
mod tests;
