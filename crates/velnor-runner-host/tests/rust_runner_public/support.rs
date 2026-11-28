// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use velnor_runner_github::EncodedJit;
use velnor_runner_host::{HostError, ReservationId, RustRunnerFailure, RustRunnerLauncher};

#[path = "../fixtures/guest.rs"]
mod guest;

const CAP_GIB: u64 = 172;
const FAKE_ORB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_orb.ts");
pub(super) const GUEST: &str =
    "-m velnor -u root /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C";
pub(super) const SECRET_JIT: &str = "eyJ2ZWxub3IiOiJzZWNyZXQtaml0LWNvbmZpZyJ9";

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) store: PathBuf,
}

impl Fixture {
    pub(super) fn new(machines: &str) -> Result<Self, Box<dyn Error>> {
        Self::build(|| machines.to_owned())
    }

    pub(super) fn ready() -> Result<Self, Box<dyn Error>> {
        Self::build(|| guest::machines(CAP_GIB, |_| {}))
    }

    pub(super) fn shaped(
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(|| guest::machines(CAP_GIB, mutate))
    }

    fn build(machines: impl FnOnce() -> String) -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let store = root.join("store");
        std::fs::create_dir_all(root.join("seen"))?;
        std::fs::create_dir_all(&store)?;
        std::fs::copy(FAKE_ORB, root.join("orb"))?;
        guest::install_guest_module(&root)?;
        std::fs::write(root.join("list.json"), machines())?;
        guest::prepare_guest(&root)?;
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700))?;
        let fixture = Self {
            _directory: directory,
            root,
            store,
        };
        fixture.configure("")?;
        Ok(fixture)
    }

    pub(super) fn configure(&self, extra: &str) -> Result<(), Box<dyn Error>> {
        let env = format!(
            "FAKE_ORB_LOG={log}\nFAKE_ORB_LIST={list}\nFAKE_ORB_STORE={store}\nFAKE_ORB_SEEN={seen}\nFAKE_ORB_PS={ps}\nFAKE_TEST_PID={pid}\n{guest}{extra}",
            log = self.root.join("orb.log").display(),
            list = self.root.join("list.json").display(),
            store = self.store.display(),
            seen = self.root.join("seen").display(),
            ps = self.root.join("ps.table").display(),
            pid = std::process::id(),
            guest = guest::environment(CAP_GIB),
        );
        std::fs::write(self.root.join("fake_orb.env"), env)?;
        Ok(())
    }

    pub(super) fn launcher(&self) -> RustRunnerLauncher {
        self.production_launcher().with_open_launch_for_tests()
    }

    pub(super) fn production_launcher(&self) -> RustRunnerLauncher {
        RustRunnerLauncher::new(
            self.root.join("orb"),
            self.store.clone(),
            self.root.join("host-runner"),
        )
    }

    pub(super) fn log(&self) -> Result<Vec<String>, Box<dyn Error>> {
        match std::fs::read_to_string(self.root.join("orb.log")) {
            Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn guest(&self, path: &str) -> PathBuf {
        self.root.join("guest").join(path)
    }

    pub(super) fn commands(&self, word: &str) -> Result<usize, Box<dyn Error>> {
        Ok(self
            .log()?
            .iter()
            .filter(|line| line.contains(&format!(" {word} ")))
            .count())
    }

    pub(super) fn ensure_refused(&self, failure: RustRunnerFailure) {
        assert_eq!(
            self.launcher().ensure_machine().err(),
            Some(HostError::RustRunner(failure))
        );
        assert!(
            !self.guest("var/lib/velnor/bin/velnor-runner").exists(),
            "a refused machine still got the runner"
        );
    }

    pub(super) fn seen(&self, name: &str) -> PathBuf {
        self.root.join("seen").join(name)
    }
}

pub(super) fn reservation(value: u64) -> Result<ReservationId, Box<dyn Error>> {
    ReservationId::new(value).ok_or_else(|| "zero reservation".into())
}

pub(super) fn jit(text: &str) -> EncodedJit {
    EncodedJit::new(text.to_owned())
}

pub(super) fn runner_lines(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|line| line.contains("/var/lib/velnor/bin/velnor-runner run"))
        .collect()
}

pub(super) async fn wait_for(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() > deadline {
            return Err(format!("{} never appeared", path.display()).into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

pub(super) fn process_exists(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
