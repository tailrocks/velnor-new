// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixListener;
use std::path::Path;

use crate::error::HostError;

#[cfg(target_os = "macos")]
pub(super) const SOCKET_PATH_LIMIT: usize = 104;
#[cfg(not(target_os = "macos"))]
pub(super) const SOCKET_PATH_LIMIT: usize = 108;

pub(super) fn bind_control_socket(path: &Path) -> Result<UnixListener, HostError> {
    check_path_length(path, SOCKET_PATH_LIMIT)?;
    UnixListener::bind(path).map_err(|error| match error.kind() {
        ErrorKind::AddrInUse => HostError::Lock,
        kind => HostError::SocketBind {
            path: leaked_path(path),
            kind,
        },
    })
}

pub(super) fn check_path_length(path: &Path, limit: usize) -> Result<(), HostError> {
    let length = path.as_os_str().as_bytes().len();
    if length >= limit {
        return Err(HostError::SocketPathTooLong {
            path: leaked_path(path),
            length,
            limit,
        });
    }
    Ok(())
}

fn leaked_path(path: &Path) -> &'static str {
    let text = path.to_str().map_or_else(
        || path.as_os_str().as_bytes().escape_ascii().to_string(),
        ToOwned::to_owned,
    );
    Box::leak(text.into_boxed_str())
}

#[cfg(test)]
#[path = "bind/tests.rs"]
mod tests;
