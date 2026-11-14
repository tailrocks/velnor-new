// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn aborted_caller_leaves_cleanup_running_and_preserves_replacements() {
    for replace in [false, true] {
        let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).unwrap();
        let directory = base.join(format!("a{:x}", u64::from_ne_bytes(random)));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let journal = directory.join("ownership.json");
        let state = Arc::new(Mutex::new(State {
            block_create: true,
            journal_path: Some(journal.clone()),
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
        runtime.block_on(async {
            let docker = bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION).unwrap();
            let session = open_qualification_session(docker, journal.clone(), "/var/lib/docker".into()).await.unwrap();
            let source = SourceIdentity { source_sha: SHA.into(), runner_version: "1".into(), docker_version: "1".into(), buildx_version: "1".into(), compose_version: "1".into(), node_version: "1".into() };
            let caller = tokio::spawn(session.qualify(source, format!("sha256:{}", "c".repeat(64)), format!("sha256:{}", "d".repeat(64)), format!("sha256:{}", "e".repeat(64)), [None, None, None], Instant::now() + Duration::from_secs(20)));
            let end = Instant::now() + Duration::from_secs(10);
            while state.lock().unwrap().accepted.is_none() {
                assert!(Instant::now() < end);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            caller.abort();
            assert!(caller.await.unwrap_err().is_cancelled());
            let name = state.lock().unwrap().accepted.clone().unwrap();
            let intent: Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
            assert_eq!(intent["settled"], json!([]));
            {
                let mut state = state.lock().unwrap();
                if replace {
                    state.resources.get_mut(&name).unwrap()["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] = json!("wrong");
                }
                state.block_create = false;
            }
            let lock_path = journal.with_extension("lock");
            let lock = std::fs::OpenOptions::new().read(true).write(true).open(lock_path).unwrap();
            loop {
                if lock.try_lock().is_ok() { break; }
                assert!(Instant::now() < end, "detached cleanup did not finish");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let state = state.lock().unwrap();
            assert!(state.resources.contains_key("unrelated-sentinel"));
            assert!(!journal.with_extension("receipt").exists());
            if replace {
                assert_eq!(state.resources[&name]["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"], "wrong");
                assert!(journal.exists());
                assert!(state.delete_snapshot.is_none());
            } else {
                assert!(!state.resources.contains_key(&name));
                assert!(!journal.exists());
                let snapshot = state.delete_snapshot.as_ref().unwrap();
                assert_eq!(snapshot["containers"][0]["id"], ID);
                assert!(snapshot["settled"].as_array().unwrap().contains(&json!(name)));
            }
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}

fn recovery_fixture(volume: bool) -> (PathBuf, String, Arc<Mutex<State>>, String) {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).unwrap();
    let directory = base.join(format!("r{:x}", u64::from_ne_bytes(random)));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let journal = directory.join("ownership.json");
    let run = "11111111111111111111111111111111";
    let name = if volume {
        format!("velnor-qualify-socket-{run}")
    } else {
        format!("velnor-qualify-runner-listener-{run}-1")
    };
    let labels = json!({"io.velnor.runner-image-qualifier.run": run, "io.velnor.runner-image-qualifier.role": if volume {"socket"} else {"runner-listener"}});
    let object = if volume {
        json!({"Name":name,"Driver":"local","Mountpoint":"/var/lib/docker/volumes/test","Labels":labels,"Scope":"local","Options":{}})
    } else {
        json!({"Id":ID,"Name":format!("/{name}"),"Config":{"Labels":labels}})
    };
    let state = Arc::new(Mutex::new(State {
        block_delete: true,
        stop_new: true,
        journal_path: Some(journal.clone()),
        ..State::default()
    }));
    {
        let mut state = state.lock().unwrap();
        state.resources.insert(name.clone(), object);
        state
            .resources
            .insert("unrelated-sentinel".into(), json!({"Id":"sentinel"}));
    }
    let record = json!({"daemon_id":"daemon-id","daemon_root":"/var/lib/docker","daemon_version":"1.0","run_id":run,
        "containers":if volume {json!([])} else {json!([{"name":name,"role":"runner-listener","id":ID}])},
        "volumes":if volume {json!([name])} else {json!([])},"settled":[name],"deleting":[]});
    std::fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    let address = directory.join("d").to_string_lossy().into_owned();
    let listener = UnixListener::bind(&address).unwrap();
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&server_state);
            std::thread::spawn(move || serve(stream, state));
        }
    });
    (directory, address, state, name)
}

#[test]
fn killed_recovery_keeps_unknown_container_and_volume_deletes_quarantined() {
    for volume in [false, true] {
        let (directory, address, state, name) = recovery_fixture(volume);
        let journal = directory.join("ownership.json");
        let mut process = child(&address, &journal);
        let end = Instant::now() + Duration::from_secs(10);
        while !state.lock().unwrap().delete_started {
            assert!(
                Instant::now() < end,
                "recovery did not send delete: {:?}",
                process.try_wait()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let snapshot: Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
        let expected = if volume {
            format!("volume:{name}")
        } else {
            format!("container:{ID}")
        };
        assert_eq!(snapshot["deleting"], json!([expected]));
        assert_eq!(state.lock().unwrap().delete_snapshot, Some(snapshot));
        process.kill().unwrap();
        process.wait().unwrap();
        let snapshot = std::fs::read(&journal).unwrap();
        let mut replacement = state.lock().unwrap().resources.remove(&name).unwrap();
        assert!(!child(&address, &journal).wait().unwrap().success());
        assert_eq!(std::fs::read(&journal).unwrap(), snapshot);
        if volume {
            replacement["Labels"]["io.velnor.runner-image-qualifier.run"] = json!("wrong");
        } else {
            replacement["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
                json!("wrong");
        }
        state
            .lock()
            .unwrap()
            .resources
            .insert(name.clone(), replacement.clone());
        assert!(!child(&address, &journal).wait().unwrap().success());
        assert_eq!(std::fs::read(&journal).unwrap(), snapshot);
        let state = state.lock().unwrap();
        assert_eq!(state.resources[&name], replacement);
        assert!(state.resources.contains_key("unrelated-sentinel"));
        assert!(!journal.with_extension("receipt").exists());
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn cancelled_recovery_keeps_lock_until_delete_response_and_cleanup() {
    for volume in [false, true] {
        let (directory, address, state, name) = recovery_fixture(volume);
        let journal = directory.join("ownership.json");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let docker =
                bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION)
                    .unwrap();
            let caller = tokio::spawn(open_qualification_session(
                docker,
                journal.clone(),
                "/var/lib/docker".into(),
            ));
            let end = Instant::now() + Duration::from_secs(10);
            while !state.lock().unwrap().delete_started {
                assert!(Instant::now() < end);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let snapshot: Value =
                serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
            assert_ne!(snapshot["deleting"], json!([]));
            caller.abort();
            assert!(caller.await.unwrap_err().is_cancelled());
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(journal.with_extension("lock"))
                .unwrap();
            assert!(lock.try_lock().is_err());
            state.lock().unwrap().block_delete = false;
            loop {
                if lock.try_lock().is_ok() {
                    break;
                }
                assert!(
                    Instant::now() < end,
                    "recovery did not finish after response"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let state = state.lock().unwrap();
            assert!(!state.resources.contains_key(&name));
            assert!(state.resources.contains_key("unrelated-sentinel"));
            assert!(!journal.with_extension("receipt").exists());
            let record: Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
            assert_eq!(record["deleting"], json!([]));
            assert_eq!(record["containers"], json!([]));
            assert_eq!(record["volumes"], json!([]));
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn timed_out_deletes_remain_unresolved_after_missing_or_replacement() {
    for volume in [false, true] {
        let (directory, address, state, name) = recovery_fixture(volume);
        let journal = directory.join("ownership.json");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let docker =
            bollard::Docker::connect_with_socket(&address, 30, bollard::API_DEFAULT_VERSION)
                .unwrap();
        assert!(
            runtime
                .block_on(open_qualification_session(
                    docker,
                    journal.clone(),
                    "/var/lib/docker".into()
                ))
                .is_err()
        );
        let snapshot = std::fs::read(&journal).unwrap();
        let record: Value = serde_json::from_slice(&snapshot).unwrap();
        assert_ne!(record["deleting"], json!([]));
        assert!(state.lock().unwrap().delete_started);
        let mut replacement = state.lock().unwrap().resources.remove(&name).unwrap();
        assert!(!child(&address, &journal).wait().unwrap().success());
        assert_eq!(std::fs::read(&journal).unwrap(), snapshot);
        if volume {
            replacement["Labels"]["io.velnor.runner-image-qualifier.run"] = json!("wrong");
        } else {
            replacement["Config"]["Labels"]["io.velnor.runner-image-qualifier.run"] =
                json!("wrong");
        }
        state
            .lock()
            .unwrap()
            .resources
            .insert(name.clone(), replacement.clone());
        assert!(!child(&address, &journal).wait().unwrap().success());
        assert_eq!(std::fs::read(&journal).unwrap(), snapshot);
        let state = state.lock().unwrap();
        assert_eq!(state.resources[&name], replacement);
        assert!(state.resources.contains_key("unrelated-sentinel"));
        assert!(!journal.with_extension("receipt").exists());
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
