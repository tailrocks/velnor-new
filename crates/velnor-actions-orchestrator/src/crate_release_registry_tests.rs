// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

struct Registry {
    replies: RefCell<VecDeque<CrateRegistryResponse>>,
    paths: RefCell<Vec<String>>,
}
impl CrateRegistryRead for Registry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        self.paths.borrow_mut().push(path.to_owned());
        self.replies
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| internal("fixture_exhausted"))
    }
}
fn response(status: u16, body: &str) -> CrateRegistryResponse {
    CrateRegistryResponse {
        status,
        body: body.as_bytes().to_vec(),
    }
}
fn fixture(replies: Vec<CrateRegistryResponse>) -> Registry {
    Registry {
        replies: RefCell::new(replies.into()),
        paths: RefCell::new(Vec::new()),
    }
}

#[test]
fn native_registry_read_obeys_shared_deadline() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read as _, Write as _};
    use std::time::{Duration, Instant};
    if std::env::var_os("VELNOR_TEST_REGISTRY_ORIGIN").is_some() {
        let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(1))?;
        let started = Instant::now();
        let result = deadline.scope(|| NativeCrateRegistryRead.get("/api/v1/crates/demo"));
        assert!(!matches!(result, Ok(Ok(_))));
        assert!(started.elapsed() < Duration::from_secs(6));
        return Ok(());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        let mut buffer = [0; 4096];
        if stream.read(&mut buffer)? == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{")?;
        let _held = hold.recv_timeout(Duration::from_secs(12));
        Ok(())
    });
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::native_registry_read_obeys_shared_deadline"
            )),
        ])
        .env("VELNOR_TEST_REGISTRY_ORIGIN", origin)
        .output()?;
    let _released = release.send(());
    worker.join().map_err(|_| "server panic")??;
    assert!(output.status.success());
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

#[test]
fn sparse_index_waits_only_for_propagation_and_binds_checksum() {
    let checksum = "a".repeat(64);
    let matching =
        serde_json::json!({"name":"demo","vers":"0.1.1","cksum":checksum,"yanked":false})
            .to_string();
    let registry = fixture(vec![response(404, ""), response(200, &matching)]);
    let waits = Cell::new(0);
    wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || {
        waits.set(waits.get() + 1);
    })
    .expect("propagation");
    assert_eq!(waits.get(), 1);
    assert_eq!(
        &*registry.paths.borrow(),
        &["/index/de/mo/demo", "/index/de/mo/demo"]
    );
    for body in [
        matching.replace(&checksum, &"b".repeat(64)),
        matching.replace("false", "true"),
        format!("{matching}\n{matching}"),
        format!(
            r#"{{"name":"demo","vers":"0.1.1","vers":"0.1.2","cksum":"{checksum}","yanked":false}}"#
        ),
        "not-json".to_owned(),
    ] {
        let registry = fixture(vec![response(200, &body)]);
        assert!(
            wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || panic!(
                "invalid response retried"
            ))
            .is_err()
        );
    }
    for status in [403, 429, 500] {
        let registry = fixture(vec![response(status, "")]);
        assert!(
            wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || panic!(
                "error retried"
            ))
            .is_err()
        );
    }
    let registry = fixture(vec![response(200, ""), response(404, "")]);
    assert!(wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || {}).is_err());
}
