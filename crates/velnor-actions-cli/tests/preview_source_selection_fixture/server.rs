// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    fs,
    path::Path,
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
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

pub(super) fn start(root: &Path, scenario: &str) -> Result<Server, Box<dyn Error>> {
    let tls = env::var_os("VELNOR_GH_TEST_TLS_ROOT").ok_or("isolated TLS fixture missing")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open("/tmp/velnor-gh-fixture.lock")?;
    lock.lock()?;
    let script = root.join("preview-server.py");
    fs::write(&script, include_str!("server.py"))?;
    let mut child = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(script)
        .arg(root)
        .arg(tls)
        .arg(scenario)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("ready").exists() {
        if let Some(status) = child.try_wait()? {
            return Err(format!("preview fixture exited: {status}").into());
        }
        if Instant::now() >= deadline {
            drop(child.kill());
            drop(child.wait());
            return Err("preview fixture startup timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(Server { child, _lock: lock })
}
