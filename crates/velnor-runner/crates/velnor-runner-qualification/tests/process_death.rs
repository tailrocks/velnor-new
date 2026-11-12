// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![expect(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::struct_excessive_bools,
    reason = "the external Docker fixture fails immediately on invalid test state"
)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use velnor_runner_qualification::{SourceIdentity, open_qualification_session};

include!("process_death/fixture.rs");

#[test]
fn child_entry() {
    let Ok(address) = std::env::var("QUALIFIER_FIXTURE_ADDRESS") else {
        return;
    };
    let journal = PathBuf::from(std::env::var_os("QUALIFIER_FIXTURE_JOURNAL").unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let docker =
        bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION).unwrap();
    let source = SourceIdentity {
        source_sha: SHA.into(),
        runner_version: "1".into(),
        docker_version: "1".into(),
        buildx_version: "1".into(),
        compose_version: "1".into(),
        node_version: "1".into(),
    };
    let result = runtime.block_on(async {
        open_qualification_session(docker, journal.clone(), "/var/lib/docker".into())
            .await?
            .qualify(
                source,
                format!("sha256:{}", "c".repeat(64)),
                format!("sha256:{}", "d".repeat(64)),
                format!("sha256:{}", "e".repeat(64)),
                [None, None, None],
                Instant::now() + Duration::from_secs(60),
            )
            .await
    });
    assert!(result.is_ok(), "{result:?}");
    std::fs::write(journal.with_extension("receipt"), b"qualified").unwrap();
}

fn child(address: &str, journal: &PathBuf) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_entry", "--nocapture"])
        .env("QUALIFIER_FIXTURE_ADDRESS", address)
        .env("QUALIFIER_FIXTURE_JOURNAL", journal)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn killed_create_recovers_only_owned_resources_and_blocks_on_cleanup_failure() {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).unwrap();
    let directory = base.join(format!(
        "q{:x}",
        u64::from_ne_bytes(random[..8].try_into().unwrap())
    ));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let journal = directory.join("ownership.json");
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
    let mut process = child(&address, &journal);
    let until = Instant::now() + Duration::from_secs(20);
    while state.lock().unwrap().accepted.is_none() {
        assert!(
            Instant::now() < until,
            "child did not submit create; status {:?}",
            process.try_wait()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(journal.exists());
    process.kill().unwrap();
    process.wait().unwrap();
    let unknown_name = state.lock().unwrap().accepted.clone().unwrap();
    let owned = {
        let mut state = state.lock().unwrap();
        state
            .resources
            .insert("unrelated-sentinel".into(), json!({"Id":"sentinel"}));
        state.stop_new = true;
        state.resources.remove(&unknown_name).unwrap()
    };
    let journal_snapshot = std::fs::read(&journal).unwrap();
    assert!(!child(&address, &journal).wait().unwrap().success());
    assert_eq!(std::fs::read(&journal).unwrap(), journal_snapshot);
    assert!(!journal.with_extension("receipt").exists());
    state
        .lock()
        .unwrap()
        .resources
        .insert(unknown_name.clone(), owned.clone());
    assert!(!child(&address, &journal).wait().unwrap().success());
    assert_eq!(std::fs::read(&journal).unwrap(), journal_snapshot);
    assert_eq!(state.lock().unwrap().resources[&unknown_name], owned);
    assert!(!journal.with_extension("receipt").exists());
    {
        let mut state = state.lock().unwrap();
        state.resources.get_mut(&unknown_name).unwrap()["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
            json!("wrong");
    }
    assert!(!child(&address, &journal).wait().unwrap().success());
    assert_eq!(std::fs::read(&journal).unwrap(), journal_snapshot);
    assert_eq!(
        state.lock().unwrap().resources[&unknown_name]["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"],
        "wrong"
    );
    assert!(!journal.with_extension("receipt").exists());

    let journal = directory.join("completed.json");
    {
        let mut state = state.lock().unwrap();
        state.accepted = None;
        state.resources.get_mut(&unknown_name).unwrap()["Id"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        state.block_create = false;
        state.block_inspect = true;
        state.stop_new = false;
    }
    let mut process = child(&address, &journal);
    let until = Instant::now() + Duration::from_secs(20);
    while !state.lock().unwrap().inspect_blocked {
        assert!(
            Instant::now() < until,
            "child did not reach inspect; status {:?}",
            process.try_wait()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let completed: Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    let name = state.lock().unwrap().accepted.clone().unwrap();
    assert_eq!(completed["containers"][0]["id"], ID);
    assert!(
        completed["settled"]
            .as_array()
            .unwrap()
            .contains(&json!(name))
    );
    process.kill().unwrap();
    process.wait().unwrap();
    {
        let mut state = state.lock().unwrap();
        state.block_inspect = false;
        state.fail_delete = true;
        state.stop_new = true;
    }
    assert!(!child(&address, &journal).wait().unwrap().success());
    assert!(journal.exists());
    assert!(!journal.with_extension("receipt").exists());
    assert!(state.lock().unwrap().resources.contains_key(&name));
    {
        let mut state = state.lock().unwrap();
        state.fail_delete = false;
        state.resources.get_mut(&name).unwrap()["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
            json!("wrong");
    }
    assert!(!child(&address, &journal).wait().unwrap().success());
    assert!(journal.exists());
    assert!(!journal.with_extension("receipt").exists());
    assert!(state.lock().unwrap().resources.contains_key(&name));
    {
        let mut state = state.lock().unwrap();
        state.resources.get_mut(&name).unwrap()["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
            completed["run_id"].clone();
    }
    assert!(!child(&address, &journal).wait().unwrap().success());
    let state = state.lock().unwrap();
    assert!(!state.resources.contains_key(&name));
    assert!(state.resources.contains_key(&unknown_name));
    assert!(state.resources.contains_key("unrelated-sentinel"));
    drop(state);
    std::fs::remove_dir_all(directory).unwrap();
}

include!("process_death/recovery.rs");
