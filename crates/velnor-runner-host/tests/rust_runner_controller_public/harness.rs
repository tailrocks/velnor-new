// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::fixtures::{TestResult, host_toml};

const FAKE_ORB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_orb.sh");
const FAKE_CURL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_curl.ts");
pub(super) const CHILD_MARKER: &str = "VELNOR_RUST_RUNNER_CONTROLLER_CHILD";

#[derive(Debug, Clone)]
pub(super) struct Event {
    pub(super) seq: u64,
    pub(super) ms: u64,
    pub(super) label: String,
    pub(super) fields: BTreeMap<String, String>,
    pub(super) text: String,
}

impl Event {
    pub(super) fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }

    pub(super) fn is_runner_start(&self) -> bool {
        self.label == "orb" && self.text.contains("/var/lib/velnor/bin/velnor-runner run")
    }

    pub(super) fn is_orb(&self, needle: &str) -> bool {
        self.label == "orb" && self.text.contains(needle)
    }
}

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

fn bun() -> TestResult<PathBuf> {
    let output = Command::new("bun")
        .args(["--print", "process.execPath"])
        .output()
        .map_err(|_| "the controller tests need Bun on PATH")?;
    let path = String::from_utf8(output.stdout)?.trim().to_owned();
    if !output.status.success() || !Path::new(&path).is_absolute() {
        return Err("bun did not report its executable".into());
    }
    Ok(PathBuf::from(path))
}

fn curl_wrapper(bun: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         input=\"$VELNOR_FAKE_DIR/seen/.curl.$$\"\n\
         cat > \"$input\"\n\
         attempt=0\n\
         while [ \"$attempt\" -lt 3 ]; do\n\
         attempt=$((attempt + 1))\n\
         {bun} \"$VELNOR_FAKE_DIR/fake_curl.ts\" < \"$input\"\n\
         status=$?\n\
         case \"$status\" in\n\
         0) rm -f \"$input\"; exit 0 ;;\n\
         3|4) rm -f \"$input\"; exit \"$status\" ;;\n\
         esac\n\
         echo \"bun exited $status (attempt $attempt)\" >> \"$VELNOR_FAKE_DIR/seen/fake-retries\"\n\
         done\n\
         rm -f \"$input\"\n\
         exit \"$status\"\n",
        bun = bun.display()
    )
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
        std::fs::copy(FAKE_ORB, world.root.join("orb"))?;
        std::fs::write(world.root.join("list.json"), r#"[{"name":"velnor"}]"#)?;
        let runner = world.store.join("bin/velnor-runner");
        std::fs::write(&runner, "")?;
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755))?;
        std::fs::copy(FAKE_CURL, world.root.join("fake_curl.ts"))?;
        let curl = world.root.join("bin/curl");
        std::fs::write(&curl, curl_wrapper(&bun()?))?;
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o700))?;
        let pat = world.root.join("pat");
        std::fs::write(&pat, format!("PAT-CANARY-{tag}"))?;
        std::fs::set_permissions(&pat, std::fs::Permissions::from_mode(0o600))?;
        world.write_config()?;
        world.write_state(scale_set_exists, &[])?;
        world.write_host_toml(max_jobs)?;
        world.configure_orb("")?;
        Ok(world)
    }

    fn write_config(&self) -> TestResult<()> {
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
        });
        std::fs::write(self.github.join("config.json"), config.to_string())?;
        Ok(())
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
        let text = host_toml(&self.root, max_jobs)?;
        let staging = self.state.join("host.toml.staging");
        std::fs::write(&staging, text)?;
        std::fs::rename(staging, self.state.join("host.toml"))?;
        Ok(())
    }

    pub(super) fn configure_orb(&self, extra: &str) -> TestResult<()> {
        let env = format!(
            "FAKE_ORB_LOG={log}\nFAKE_ORB_LIST={list}\nFAKE_ORB_STORE={store}\nFAKE_ORB_SEEN={seen}\nFAKE_ORB_PS={ps}\nFAKE_REQUESTS={requests}\n{extra}",
            log = self.root.join("orb.log").display(),
            list = self.root.join("list.json").display(),
            store = self.store.display(),
            seen = self.seen.display(),
            ps = self.root.join("ps.table").display(),
            requests = self.seen.join("requests").display(),
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
        let child = Command::new(std::env::current_exe()?)
            .args(["--exact", test, "--nocapture"])
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

    pub(super) fn finish(&mut self, seconds: u64) -> TestResult<String> {
        let started = Instant::now();
        let mut child = self.child.take().ok_or("no controller child")?;
        let deadline = started + Duration::from_secs(seconds);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() > deadline {
                self.child = Some(child);
                return Err(format!("the controller did not exit within {seconds} s").into());
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

    pub(super) fn heartbeat_ms(&self) -> Option<u64> {
        let text = std::fs::read_to_string(self.state.join("rust-runner-controller.json")).ok()?;
        serde_json::from_str::<Value>(&text).ok()?["updated_unix_ms"].as_u64()
    }

    pub(super) fn heartbeat_phase(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.state.join("rust-runner-controller.json")).ok()?;
        serde_json::from_str::<Value>(&text).ok()?["phase"]
            .as_str()
            .map(str::to_owned)
    }

    fn cleanup(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.seen) {
            let names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            for name in &names {
                if let Some(jit) = name.strip_suffix(".pid") {
                    let _ = std::fs::write(self.seen.join(format!("release-{jit}")), "");
                }
            }
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            for name in &names {
                if name.strip_suffix(".pid").is_some()
                    && let Ok(pid) = std::fs::read_to_string(self.seen.join(name))
                {
                    self.kill_if_ours(pid.trim());
                }
            }
        }
    }

    fn kill_if_ours(&self, pid: &str) {
        let Ok(output) = Command::new("ps").args(["-o", "args=", "-p", pid]).output() else {
            return;
        };
        let args = String::from_utf8_lossy(&output.stdout);
        if output.status.success()
            && args
                .trim()
                .starts_with(&format!("/bin/sh {}/orb ", self.root.display()))
        {
            let _ = Command::new("kill").args(["-9", pid]).status();
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if std::thread::panicking() {
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
        } else if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn wait_for(what: &str, seconds: u64, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn parse_events(path: &Path) -> Vec<Event> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_event).collect()
}

fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(4, ' ');
    let seq = parts.next()?.parse().ok()?;
    let ms = parts.next()?.parse().unwrap_or(0);
    let kind = parts.next()?;
    let rest = parts.next().unwrap_or("");
    if kind == "ORB" {
        return Some(Event {
            seq,
            ms,
            label: "orb".to_owned(),
            fields: BTreeMap::new(),
            text: rest.to_owned(),
        });
    }
    let (label, detail) = rest.split_once(' ').unwrap_or((rest, ""));
    let fields = detail
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    Some(Event {
        seq,
        ms,
        label: label.to_owned(),
        fields,
        text: detail.to_owned(),
    })
}

pub(super) fn files_containing(root: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        } else if metadata.is_file()
            && std::fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            })
        {
            found.push(path);
        }
    }
    found
}

pub(super) fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
