// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::{SOCKET_PATH_LIMIT, bind_control_socket, check_path_length};
use crate::config::HostConfig;
use crate::controller_ipc::server::ControlServer;
use crate::error::HostError;
use crate::journal::Journal;

const CONFIG: &str = "schema = 2\n\
    [[connections]]\n\
    connection_id = \"lab\"\n\
    repository_id = 42\n\
    repository = \"tailrocks/velnor-lab\"\n\
    credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
    [connections.scale_set]\n\
    name = \"ubuntu-26.04-arm-scale-set\"\n\
    labels = [\"velnor\", \"ubuntu-26.04-arm-scale-set\"]\n\
    [connections.source_policy]\n\
    mode = \"branch_restricted\"\n\
    branches = [\"main\"]\n\
    tag_prefixes = []\n\
    events = [\"push\"]\n\
    pull_request_rules = []\n\
    [host]\n\
    max_jobs = 1\n\
    [host.resources]\n\
    runner_cpu_millicores = 2000\n\
    runner_memory_bytes = 4294967296\n\
    dind_cpu_millicores = 1000\n\
    dind_memory_bytes = 2147483648\n\
    operational_memory_reserve_bytes = 1073741824\n\
    [docker]\n\
    context = \"orbstack\"\n\
    platform = \"linux/arm64\"\n\
    endpoint = \"unix:///var/run/docker.sock\"\n";

type TestResult = Result<(), Box<dyn Error>>;

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
fn a_path_of_exactly_the_limit_is_too_long_and_one_byte_less_is_not() {
    let at_limit = bind_error(&path_of_length(SOCKET_PATH_LIMIT));
    assert!(matches!(
        at_limit,
        Some(HostError::SocketPathTooLong { length, .. }) if length == SOCKET_PATH_LIMIT
    ));
    assert_eq!(
        check_path_length(&path_of_length(SOCKET_PATH_LIMIT - 1), SOCKET_PATH_LIMIT),
        Ok(())
    );
}

#[test]
fn paths_that_differ_only_in_non_utf8_bytes_report_different_paths() {
    let named = |byte: u8| {
        let mut bytes = path_of_length(SOCKET_PATH_LIMIT + 4)
            .into_os_string()
            .into_vec();
        bytes.push(byte);
        PathBuf::from(OsString::from_vec(bytes))
    };
    let first = bind_error(&named(0xfe)).map(|error| error.to_string());
    let second = bind_error(&named(0xff)).map(|error| error.to_string());
    assert!(first.is_some());
    assert_ne!(first, second);
}

#[test]
fn a_bind_failure_other_than_a_taken_address_keeps_its_real_cause() -> TestResult {
    let directory = tempfile::tempdir()?;
    let error = bind_error(&directory.path().join("absent/c.sock"));
    assert!(matches!(
        error,
        Some(HostError::SocketBind {
            kind: std::io::ErrorKind::NotFound,
            ..
        })
    ));
    Ok(())
}

#[test]
fn a_taken_address_is_still_a_held_lock() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("c.sock");
    let _first = bind_control_socket(&path)?;
    assert_eq!(bind_error(&path), Some(HostError::Lock));
    Ok(())
}

#[tokio::test]
async fn controller_startup_with_a_long_state_path_reports_the_length_not_a_held_lock() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let mut state = directory.path().to_owned();
    while state.as_os_str().len() < SOCKET_PATH_LIMIT {
        state.push("deeper");
    }
    let journal = Journal::open(&directory.path().join("journal.db")).await?;
    let config = HostConfig::parse(CONFIG)?;
    let result = ControlServer::start(&state, journal, &config, String::new(), String::new());
    assert!(matches!(
        result.err(),
        Some(HostError::SocketPathTooLong { .. })
    ));
    Ok(())
}
