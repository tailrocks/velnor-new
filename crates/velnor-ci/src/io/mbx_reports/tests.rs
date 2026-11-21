// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::collect;
use super::inside_session;
use super::session_lease_is_held;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn an_entry_that_cannot_be_listed_is_unknown_evidence() {
    let entries = std::iter::once(Err(io::Error::other("entry vanished")));
    assert_eq!(collect(entries).map(|reports| reports.unlisted), Some(1));
}

struct Dir(PathBuf);

impl Dir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = PathBuf::from("/tmp")
            .join(format!("velnor-mbx-{label}-{}-{nonce}", std::process::id()));
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
fn a_socket_left_by_a_dead_listener_is_not_a_nested_session() {
    let dir = Dir::new("dropped");
    let path = dir.0.join("mbx.sock");
    drop(UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}")));
    assert!(!inside_session(Some(path.as_os_str())));
}

#[test]
fn live_socket_is_a_nested_session() {
    let dir = Dir::new("live");
    let path = dir.0.join("mbx.sock");
    let _listener = UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(inside_session(Some(path.as_os_str())));
}

fn held_lease(dir: &Dir) -> File {
    let lease = File::create(dir.0.join("owner.lock")).unwrap_or_else(|e| unreachable!("{e}"));
    lease.lock().unwrap_or_else(|e| unreachable!("{e}"));
    lease
}

#[test]
fn a_lease_held_through_another_handle_is_held() {
    let dir = Dir::new("lease-held");
    let _holder = held_lease(&dir);
    assert!(session_lease_is_held(&dir.0.join("cache-agent.sock")));
}

#[test]
fn an_unlocked_lease_is_not_held_and_stays_unlocked() {
    let dir = Dir::new("lease-free");
    drop(held_lease(&dir));
    let socket = dir.0.join("cache-agent.sock");
    assert!(!session_lease_is_held(&socket));
    let probe = File::open(dir.0.join("owner.lock")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(probe.try_lock().is_ok());
}

#[test]
fn a_missing_lease_is_not_held_and_is_not_created() {
    let dir = Dir::new("lease-missing");
    assert!(!session_lease_is_held(&dir.0.join("cache-agent.sock")));
    assert!(!dir.0.join("owner.lock").exists());
}

#[test]
fn a_symlinked_lease_is_not_held_even_when_its_target_is_locked() {
    let dir = Dir::new("lease-linked");
    let target = dir.0.join("target.lock");
    let holder = File::create(&target).unwrap_or_else(|e| unreachable!("{e}"));
    holder.lock().unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, dir.0.join("owner.lock")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!session_lease_is_held(&dir.0.join("cache-agent.sock")));
}

#[test]
fn a_refused_socket_whose_lease_is_held_is_a_nested_session() {
    let dir = Dir::new("refused-held");
    let path = dir.0.join("cache-agent.sock");
    drop(UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}")));
    let _holder = held_lease(&dir);
    assert!(inside_session(Some(path.as_os_str())));
}

#[test]
fn a_refused_socket_whose_lease_was_released_is_not_a_nested_session() {
    let dir = Dir::new("refused-released");
    let path = dir.0.join("cache-agent.sock");
    drop(UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}")));
    drop(held_lease(&dir));
    assert!(!inside_session(Some(path.as_os_str())));
}

#[test]
fn a_refused_socket_with_a_symlinked_held_lease_is_not_a_nested_session() {
    let dir = Dir::new("refused-linked");
    let path = dir.0.join("cache-agent.sock");
    drop(UnixListener::bind(&path).unwrap_or_else(|e| unreachable!("{e}")));
    let target = dir.0.join("target.lock");
    let holder = File::create(&target).unwrap_or_else(|e| unreachable!("{e}"));
    holder.lock().unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, dir.0.join("owner.lock")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!inside_session(Some(path.as_os_str())));
}
