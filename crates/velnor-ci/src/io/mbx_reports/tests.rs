// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::inside_session;
use std::ffi::OsStr;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Dir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("velnor-mbx-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        Self(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unset_socket_is_not_a_nested_session() {
    assert!(!inside_session(None));
}

#[test]
fn empty_socket_is_not_a_nested_session() {
    assert!(!inside_session(Some(OsStr::new(""))));
}

#[test]
fn leftover_socket_path_without_a_socket_is_not_a_nested_session() {
    let dir = Dir::new("stale");
    let missing = dir.0.join("missing.sock");
    assert!(!inside_session(Some(missing.as_os_str())));
    let regular = dir.0.join("regular.sock");
    std::fs::write(&regular, b"").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!inside_session(Some(regular.as_os_str())));
}

#[test]
fn live_socket_is_a_nested_session() {
    let dir = Dir::new("live");
    let path = dir.0.join("mbx.sock");
    let _listener = UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(inside_session(Some(path.as_os_str())));
}

#[test]
fn stale_socket_file_without_a_listener_is_not_a_nested_session() {
    let dir = Dir::new("dropped");
    let path = dir.0.join("mbx.sock");
    let listener = UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}"));
    drop(listener);
    assert!(path.exists());
    assert!(!inside_session(Some(path.as_os_str())));
}
