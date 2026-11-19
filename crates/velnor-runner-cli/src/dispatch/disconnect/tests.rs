// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::path::Path;
use std::process::ExitCode;

use velnor_runner_host::{ConnectionId, CreatedSetDeletion, HostError};

use super::disconnect_with_detach;
use crate::dispatch::connection_state::{connection_drain_proof, persist_host, read_host};

const CONNECTION: &str = "conn-test-1";
const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Run {
    exit: ExitCode,
    deletes: u32,
    configured_at_delete: Option<bool>,
    detaches: u32,
}

fn configured(state: &Path) -> bool {
    let id = ConnectionId::parse(CONNECTION).ok();
    match (read_host(state), id) {
        (Ok(Some(config)), Some(id)) => config.connection(&id).is_some(),
        _ => false,
    }
}

fn disconnect(
    state: &Path,
    answer: Result<CreatedSetDeletion, HostError>,
) -> Result<Run, Box<dyn std::error::Error>> {
    persist_host(state, CONFIG).map_err(|_| "host.toml")?;
    let deletes = Cell::new(0_u32);
    let configured_at_delete = Cell::new(None);
    let detaches = Cell::new(0_u32);
    let exit = disconnect_with_detach(
        state,
        CONNECTION,
        true,
        |_, _, _| Ok(true),
        |_, _, _| {
            detaches.set(detaches.get() + 1);
            Ok(true)
        },
        |state, _, _| {
            deletes.set(deletes.get() + 1);
            configured_at_delete.set(Some(configured(state)));
            answer
        },
    );
    Ok(Run {
        exit,
        deletes: deletes.get(),
        configured_at_delete: configured_at_delete.get(),
        detaches: detaches.get(),
    })
}

fn succeeded(exit: ExitCode) -> bool {
    format!("{exit:?}") == format!("{:?}", ExitCode::SUCCESS)
}

fn id() -> Result<ConnectionId, Box<dyn std::error::Error>> {
    Ok(ConnectionId::parse(CONNECTION)?)
}

#[test]
fn a_created_set_is_deleted_before_the_connection_leaves_host_toml() -> TestResult {
    let dir = tempfile::tempdir()?;
    let state = dir.path().canonicalize()?;
    let run = disconnect(&state, Ok(CreatedSetDeletion::Deleted))?;
    assert!(succeeded(run.exit));
    assert_eq!(run.deletes, 1);
    assert_eq!(run.configured_at_delete, Some(true));
    assert_eq!(run.detaches, 1);
    assert!(!configured(&state));
    assert!(connection_drain_proof(&state, &id()?).exists());
    Ok(())
}

#[test]
fn an_adopted_set_detaches_without_a_delete_result() -> TestResult {
    let dir = tempfile::tempdir()?;
    let state = dir.path().canonicalize()?;
    let run = disconnect(&state, Ok(CreatedSetDeletion::NotOwned))?;
    assert!(succeeded(run.exit));
    assert!(!configured(&state));
    Ok(())
}

#[test]
fn any_unproven_delete_keeps_the_connection_configured_and_draining() -> TestResult {
    let answers: [Result<CreatedSetDeletion, HostError>; 5] = [
        Ok(CreatedSetDeletion::Refused(403)),
        Ok(CreatedSetDeletion::Uncertain),
        Ok(CreatedSetDeletion::TimedOut),
        Ok(CreatedSetDeletion::ControllerOffline),
        Err(HostError::Lock),
    ];
    for answer in answers {
        let dir = tempfile::tempdir()?;
        let state = dir.path().canonicalize()?;
        let run = disconnect(&state, answer)?;
        assert!(!succeeded(run.exit));
        assert_eq!(run.deletes, 1);
        assert_eq!(run.detaches, 0);
        assert!(configured(&state));
        assert!(!connection_drain_proof(&state, &id()?).exists());
    }
    Ok(())
}
