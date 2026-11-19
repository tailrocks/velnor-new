// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![expect(
    clippy::unwrap_used,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    reason = "the external Docker fixture fails immediately on invalid test state"
)]
#![allow(
    dead_code,
    reason = "the shared fixture serves more tests than this file runs"
)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use velnor_runner_qualification::{QualificationError, qualify_probe};

include!("process_death/fixture.rs");

#[test]
fn unjournaled_probe_create_without_response_removes_the_owned_container() {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).unwrap();
    let directory = base.join(format!("u{:x}", u64::from_ne_bytes(random)));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = Arc::new(Mutex::new(State {
        block_create: true,
        ..State::default()
    }));
    state
        .lock()
        .unwrap()
        .resources
        .insert("unrelated-sentinel".into(), json!({"Id":"sentinel"}));
    let address = directory.join("d").to_string_lossy().into_owned();
    let listener = UnixListener::bind(&address).unwrap();
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&server_state);
            std::thread::spawn(move || serve(stream, state));
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(async {
        let docker =
            bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION)
                .unwrap();
        qualify_probe(
            &docker,
            &format!("sha256:{}", "c".repeat(64)),
            "/var/lib/docker",
            None,
            Instant::now() + Duration::from_secs(1),
        )
        .await
    });
    let mut locked = state.lock().unwrap();
    locked.block_create = false;
    let name = locked.accepted.clone().unwrap();
    assert!(
        matches!(result, Err(QualificationError::ContainerCreateUncertain)),
        "{result:?}"
    );
    assert!(
        !locked.resources.contains_key(&name),
        "the owned container from the uncertain create must be removed"
    );
    assert!(locked.resources.contains_key("unrelated-sentinel"));
    drop(locked);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unjournaled_probe_create_does_not_remove_a_container_with_foreign_labels() {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).unwrap();
    let directory = base.join(format!("v{:x}", u64::from_ne_bytes(random)));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = Arc::new(Mutex::new(State {
        block_create: true,
        ..State::default()
    }));
    let address = directory.join("d").to_string_lossy().into_owned();
    let listener = UnixListener::bind(&address).unwrap();
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&server_state);
            std::thread::spawn(move || serve(stream, state));
        }
    });
    let watcher_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            let mut locked = watcher_state.lock().unwrap();
            if let Some(name) = locked.accepted.clone() {
                locked.resources.get_mut(&name).unwrap()["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
                    json!("wrong");
                return;
            }
            drop(locked);
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(async {
        let docker =
            bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION)
                .unwrap();
        qualify_probe(
            &docker,
            &format!("sha256:{}", "c".repeat(64)),
            "/var/lib/docker",
            None,
            Instant::now() + Duration::from_secs(1),
        )
        .await
    });
    let mut locked = state.lock().unwrap();
    locked.block_create = false;
    let name = locked.accepted.clone().unwrap();
    assert!(result.is_err());
    assert!(locked.resources.contains_key(&name));
    drop(locked);
    std::fs::remove_dir_all(directory).unwrap();
}
