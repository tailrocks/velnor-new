// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::{SOCKET_PATH_LIMIT, bind_control_socket};
use crate::error::HostError;

fn path_of_length(length: usize) -> PathBuf {
    let mut text = String::from("/");
    while text.len() < length {
        text.push('a');
    }
    PathBuf::from(text)
}

fn bind_error(path: &Path) -> Option<HostError> {
    bind_control_socket(path).err()
}

#[test]
fn a_150_byte_state_path_names_the_path_length_and_limit() {
    let path = path_of_length(150);
    let error = bind_error(&path);
    assert!(matches!(
        error,
        Some(HostError::SocketPathTooLong { length: 150, limit, .. }) if limit == SOCKET_PATH_LIMIT
    ));
    let message = error.map(|error| error.to_string()).unwrap_or_default();
    assert!(message.contains(&path.display().to_string()));
    assert!(message.contains("150 bytes"));
    assert!(message.contains(&format!("{SOCKET_PATH_LIMIT}-byte limit")));
    assert!(message.contains("shorter --state"));
}

#[test]
fn a_bind_failure_other_than_a_taken_address_keeps_its_real_cause() {
    let error = bind_error(Path::new("/nonexistent-velnor-dir/c.sock"));
    assert!(matches!(
        error,
        Some(HostError::SocketBind {
            kind: std::io::ErrorKind::NotFound,
            ..
        })
    ));
}

#[test]
fn a_taken_address_is_still_a_held_lock() {
    let Ok(directory) = tempfile::tempdir() else {
        return;
    };
    let path = directory.path().join("c.sock");
    let first = bind_control_socket(&path);
    assert!(first.is_ok());
    assert_eq!(bind_error(&path), Some(HostError::Lock));
}
