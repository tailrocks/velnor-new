// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs, io,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use super::Fixture;

pub(super) fn start_for_isolated_worker(fixture: &Fixture) -> io::Result<Option<Server>> {
    if std::env::var("VELNOR_GH_FIXTURE_TARGET").as_deref() != Ok("image_publication") {
        return Ok(None);
    }
    Server::start(fixture).map(Some)
}

pub(super) struct Server {
    child: Child,
    _lock: fs::File,
}

impl Server {
    fn start(fixture: &Fixture) -> io::Result<Self> {
        let home = fixture
            .state
            .parent()
            .ok_or_else(|| io::Error::other("fixture home missing"))?;
        let tls = std::env::var_os("VELNOR_GH_TEST_TLS_ROOT")
            .ok_or_else(|| io::Error::other("TLS fixture missing"))?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open("/tmp/velnor-image-api.lock")?;
        lock.lock()?;
        let script = home.join("api-server.py");
        let ready = home.join("api-ready");
        match fs::remove_file(&ready) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::write(&script, include_str!("api_server.py"))?;
        let child = Command::new("/usr/bin/python3")
            .args([
                script.as_os_str(),
                home.as_os_str(),
                &tls,
                fixture.mise_bin.as_os_str(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut server = Self { child, _lock: lock };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if let Some(status) = server.child.try_wait()? {
                return Err(io::Error::other(format!("GitHub fixture exited: {status}")));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("GitHub fixture startup timed out"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}
