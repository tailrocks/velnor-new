// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Identities, Paths};
use std::{
    error::Error,
    fs,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) struct Server {
    child: Child,
    _lock: fs::File,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn start(paths: &Paths, ids: &Identities) -> Result<Server, Box<dyn Error>> {
    let tls = std::env::var_os("VELNOR_GH_TEST_TLS_ROOT").ok_or("isolated TLS fixture missing")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open("/tmp/velnor-gh-fixture.lock")?;
    lock.lock()?;
    let script = paths.root.join("provider/server.py");
    fs::write(&script, include_str!("server.py"))?;
    let mut child = Command::new("/usr/bin/python3")
        .arg(script)
        .arg(paths.root.join("provider"))
        .arg(tls)
        .arg(&ids.base)
        .arg(&ids.head)
        .arg(&ids.merge)
        .arg(&ids.root_tree)
        .arg(&ids.vel_nor_tree)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let ready = paths.root.join("provider/ready");
    while !ready.exists() {
        if let Some(status) = child.try_wait()? {
            return Err(format!("TLS fixture exited: {status}").into());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("TLS fixture startup timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(Server { child, _lock: lock })
}
