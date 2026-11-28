// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::events::{Event, parse_events, wait_for, wait_unless};
use super::fixtures::{TestResult, bun, curl_wrapper, host_toml, organization_host_toml};

const FAKE_ORB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_orb.ts");
const FAKE_GITHUB: [&str; 6] = [
    "fake_curl.ts",
    "fake_github_admin.ts",
    "fake_github_api.ts",
    "fake_github_core.ts",
    "fake_github_queue.ts",
    "fake_github_shapes.ts",
];
const FINISH_MARGIN: Duration = Duration::from_secs(15);
pub(super) const CHILD_MARKER: &str = "VELNOR_RUST_RUNNER_CONTROLLER_CHILD";

pub(super) struct World {
    _directory: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) store: PathBuf,
    pub(super) state: PathBuf,
    pub(super) seen: PathBuf,
    pub(super) github: PathBuf,
    pub(super) tag: &'static str,
    child: Option<Child>,
}

impl World {
    pub(super) fn new(
        tag: &'static str,
        max_jobs: u32,
        scale_set_exists: bool,
    ) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let world = Self {
            store: root.join("store"),
            state: root.join("state"),
            seen: root.join("seen"),
            github: root.join("github"),
            root,
            tag,
            child: None,
            _directory: directory,
        };
        for path in [
            world.store.join("bin"),
            world.state.clone(),
            world.seen.clone(),
            world.github.join("messages"),
            world.github.join("jits"),
            world.root.join("bin"),
        ] {
            std::fs::create_dir_all(path)?;
        }
        std::fs::set_permissions(&world.store, std::fs::Permissions::from_mode(0o700))?;
        std::fs::copy(FAKE_ORB, world.root.join("orb"))?;
        std::fs::write(
            world.root.join("list.json"),
            r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#,
        )?;
        let runner = world.store.join("bin/velnor-runner");
        std::fs::write(&runner, "")?;
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755))?;
        for name in FAKE_GITHUB {
            std::fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(name),
                world.root.join(name),
            )?;
        }
        let curl = world.root.join("bin/curl");
        std::fs::write(&curl, curl_wrapper(&bun()?))?;
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o700))?;
        let pat = world.root.join("pat");
        std::fs::write(&pat, format!("PAT-CANARY-{tag}"))?;
        std::fs::set_permissions(&pat, std::fs::Permissions::from_mode(0o600))?;
        world.write_config("main")?;
        world.write_state(scale_set_exists, &[])?;
        world.write_host_toml(max_jobs)?;
        world.configure_orb("")?;
        Ok(world)
    }

    pub(super) fn write_config(&self, default_branch: &str) -> TestResult<()> {
        let config = json!({
            "owner": "tailrocks",
            "repo": "velnor-lab",
            "repository_id": 42,
            "scale_set": "velnor-mac",
            "labels": ["velnor", "velnor-mac"],
            "canary_tag": self.tag,
            "queue_url": "https://queue.example.com/messages",
            "admin_url": "https://pipelines.example.com/",
            "store": self.store,
            "ps_table": self.root.join("ps.table"),
            "default_branch": default_branch,
        });
        std::fs::write(self.github.join("config.json"), config.to_string())?;
        Ok(())
    }

    pub(super) fn use_organization(&self) -> TestResult<()> {
        let path = self.github.join("config.json");
        let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        config["organization"] = json!("tailrocks");
        std::fs::write(path, config.to_string())?;
        self.write_host_toml_text(&organization_host_toml(&self.root, 1)?)
    }

    pub(super) fn write_state(
        &self,
        scale_set_exists: bool,
        runners: &[(&str, u64)],
    ) -> TestResult<()> {
        let planted: Vec<Value> = runners
            .iter()
            .map(|(name, id)| json!({"id": id, "name": name, "runnerScaleSetId": 7}))
            .collect();
        let state = json!({
            "scale_set_id": scale_set_exists.then_some(7),
            "sessions": [],
            "runners": planted,
            "deleted": [],
            "attempts": {},
            "next_runner_id": 100,
            "next_session": 1,
        });
        std::fs::write(self.github.join("state.json"), state.to_string())?;
        Ok(())
    }

    pub(super) fn write_host_toml(&self, max_jobs: u32) -> TestResult<()> {
        self.write_host_toml_text(&host_toml(&self.root, max_jobs)?)
    }

    pub(super) fn write_host_toml_text(&self, text: &str) -> TestResult<()> {
        let staging = self.state.join("host.toml.staging");
        std::fs::write(&staging, text)?;
        std::fs::rename(staging, self.state.join("host.toml"))?;
        Ok(())
    }

    pub(super) fn configure_orb(&self, extra: &str) -> TestResult<()> {
        let env = format!(
            "FAKE_ORB_LOG={log}\nFAKE_ORB_LIST={list}\nFAKE_ORB_STORE={store}\nFAKE_ORB_SEEN={seen}\nFAKE_ORB_PS={ps}\nFAKE_REQUESTS={requests}\nFAKE_TEST_PID={pid}\n{extra}",
            log = self.root.join("orb.log").display(),
            list = self.root.join("list.json").display(),
            store = self.store.display(),
            seen = self.seen.display(),
            ps = self.root.join("ps.table").display(),
            requests = self.seen.join("requests").display(),
            pid = std::process::id(),
        );
        let staging = self.root.join("fake_orb.env.staging");
        std::fs::write(&staging, env)?;
        std::fs::rename(staging, self.root.join("fake_orb.env"))?;
        Ok(())
    }

    pub(super) fn queue_message(&self, id: u64, offers: &[Value]) -> TestResult<()> {
        let staging = self.github.join(format!("messages/{id}.tmp"));
        std::fs::write(&staging, json!({"id": id, "offers": offers}).to_string())?;
        std::fs::rename(staging, self.github.join(format!("messages/{id}.json")))?;
        Ok(())
    }

    pub(super) fn script_refusals(&self, script: &Value) -> TestResult<()> {
        std::fs::write(self.github.join("script.json"), script.to_string())?;
        Ok(())
    }

    pub(super) fn spawn_controller(
        &mut self,
        test: &str,
        extra: &[(&str, &str)],
    ) -> TestResult<()> {
        let path = format!(
            "{}:{}",
            self.root.join("bin").display(),
            std::env::var("PATH")?
        );
        std::fs::create_dir_all(self.root.join("tmp"))?;
        let child = Command::new(std::env::current_exe()?)
            .args(["--exact", test, "--nocapture"])
            .env("TMPDIR", self.root.join("tmp"))
            .env(CHILD_MARKER, &self.root)
            .env("PATH", path)
            .env("VELNOR_FAKE_DIR", &self.root)
            .envs(extra.iter().copied())
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(self.root.join("child.out"))?)
            .stderr(std::fs::File::create(self.root.join("child.err"))?)
            .spawn()?;
        self.child = Some(child);
        Ok(())
    }

    pub(super) fn stop(&self) -> TestResult<()> {
        std::fs::write(self.seen.join("stop"), "")?;
        Ok(())
    }

    pub(super) fn events(&self) -> Vec<Event> {
        parse_events(&self.seen.join("requests"))
    }

    pub(super) fn gh(&self, label: &str) -> Vec<Event> {
        self.events()
            .into_iter()
            .filter(|event| event.label == label)
            .collect()
    }

    pub(super) fn wait_for_event(
        &self,
        what: &str,
        seconds: u64,
        check: impl Fn(&[Event]) -> bool,
    ) {
        wait_for(what, seconds, || check(&self.events()));
    }

    pub(super) fn wait_for_event_unless(
        &self,
        what: &str,
        seconds: u64,
        check: impl Fn(&[Event]) -> bool,
        violation: impl Fn(&[Event]) -> Option<String>,
    ) {
        wait_unless(
            what,
            seconds,
            || check(&self.events()),
            || violation(&self.events()),
        );
    }

    pub(super) fn finish(&mut self) -> TestResult<String> {
        let started = Instant::now();
        let mut child = self.child.take().ok_or("no controller child")?;
        let deadline = started + finish_budget();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() > deadline {
                self.child = Some(child);
                return Err(
                    format!("the controller did not exit within {:?}", finish_budget()).into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let stdout = std::fs::read_to_string(self.root.join("child.out"))?;
        assert!(stdout.contains("running 1 test"), "{stdout}");
        assert!(
            status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}\n{}",
            std::fs::read_to_string(self.root.join("child.err")).unwrap_or_default()
        );
        let exit = stdout
            .lines()
            .find_map(|line| line.strip_prefix("CONTROLLER_EXIT "))
            .ok_or("the controller printed no exit")?
            .to_owned();
        println!("controller exit {exit} after {:?}", started.elapsed());
        Ok(exit)
    }

    pub(super) fn child_stderr(&self) -> String {
        std::fs::read_to_string(self.root.join("child.err")).unwrap_or_default()
    }

    pub(super) fn pid_of(&self, jit_name: &str) -> Option<String> {
        std::fs::read_to_string(self.seen.join(format!("{jit_name}.pid")))
            .ok()
            .map(|text| text.trim().to_owned())
    }

    pub(super) fn release(&self, jit_name: &str) -> TestResult<()> {
        std::fs::write(self.seen.join(format!("release-{jit_name}")), "")?;
        Ok(())
    }

    fn heartbeat(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.state.join("rust-runner-controller.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub(super) fn heartbeat_ms(&self) -> Option<u64> {
        self.heartbeat()?["updated_unix_ms"].as_u64()
    }

    pub(super) fn heartbeat_hash(&self) -> Option<String> {
        self.heartbeat()?["config_sha256"]
            .as_str()
            .map(str::to_owned)
    }

    pub(super) fn heartbeat_phase(&self) -> Option<String> {
        self.heartbeat()?["phase"].as_str().map(str::to_owned)
    }

    pub(super) fn terminate(&self) -> TestResult<()> {
        let child = self.child.as_ref().ok_or("no controller child")?;
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()?;
        Ok(())
    }

    fn cleanup(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.seen) {
            for entry in entries.filter_map(Result::ok) {
                if let Some(jit) = entry.file_name().to_string_lossy().strip_suffix(".pid") {
                    let _ = std::fs::write(self.seen.join(format!("release-{jit}")), "");
                }
            }
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.kill_grandchildren();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !self.kill_orbs().is_empty() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn kill_grandchildren(&self) {
        let Ok(entries) = std::fs::read_dir(&self.seen) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("grandchild.")
                && let Ok(pid) = std::fs::read_to_string(entry.path())
            {
                let _ = Command::new("kill").args(["-9", pid.trim()]).status();
            }
        }
    }

    fn kill_orbs(&self) -> Vec<String> {
        let Ok(output) = Command::new("ps").args(["-axo", "pid=,args="]).output() else {
            return Vec::new();
        };
        let orb = format!("{}/orb", self.root.display());
        let found: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().split_once(' '))
            .filter(|(_, args)| args.split_whitespace().nth(1) == Some(orb.as_str()))
            .map(|(pid, _)| pid.to_owned())
            .collect();
        for pid in &found {
            let _ = Command::new("kill").args(["-9", pid]).status();
        }
        found
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.cleanup();
        eprintln!(
            "--- requests ---\n{}",
            std::fs::read_to_string(self.seen.join("requests")).unwrap_or_default()
        );
        eprintln!(
            "--- child stdout ---\n{}",
            std::fs::read_to_string(self.root.join("child.out")).unwrap_or_default()
        );
        eprintln!("--- child stderr ---\n{}", self.child_stderr());
    }
}

fn finish_budget() -> Duration {
    velnor_runner_host::CONTROLLER_STOP_BUDGET + FINISH_MARGIN
}

#[test]
fn the_finish_budget_outlasts_the_controller_stop() {
    assert!(finish_budget() > velnor_runner_host::CONTROLLER_STOP_BUDGET);
}
