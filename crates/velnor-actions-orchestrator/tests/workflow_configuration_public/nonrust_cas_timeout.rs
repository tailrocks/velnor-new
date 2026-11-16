// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use super::{
    nonrust_tools_roundtrip::RenderedCommand,
    tools_cache_verify_hook::{Captured, Sandbox},
};

struct OwnedVerifier(Child);

impl Drop for OwnedVerifier {
    fn drop(&mut self) {
        if let Err(error) = self.0.kill() {
            eprintln!("owned verifier cleanup kill: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("owned verifier cleanup wait: {error}");
        }
    }
}

struct StoppedHash {
    pid: u32,
    owner: u32,
    artifact: PathBuf,
    armed: bool,
}

impl Drop for StoppedHash {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = self.terminate()
        {
            eprintln!("owned hasher cleanup kill: {error}");
        }
    }
}

impl StoppedHash {
    fn terminate(&self) -> Result<(), Box<dyn Error>> {
        if owned_hash(self.owner, &self.artifact)? == Some(self.pid) {
            let status = Command::new("/bin/kill")
                .args(["-KILL", &self.pid.to_string()])
                .status()?;
            if !status.success() {
                return Err("owned hasher kill failed".into());
            }
        }
        Ok(())
    }
}

struct Probe {
    child: OwnedVerifier,
    output: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Probe {
    fn start(
        runner: &Sandbox,
        step: &RenderedCommand,
        env: &[(String, String)],
    ) -> Result<Self, Box<dyn Error>> {
        let script = runner.workspace.path().join("observed-verifier.sh");
        let output = runner.workspace.path().join("observed-output");
        let stdout = runner.workspace.path().join("observed-stdout");
        let stderr = runner.workspace.path().join("observed-stderr");
        fs::write(&script, &step.run)?;
        fs::write(&output, "")?;
        let mut words = step.shell.split_whitespace();
        let executable = words.next().ok_or("no verifier executable")?;
        let child = OwnedVerifier(
            Command::new(executable)
                .args(words.map(|word| word.replace("{0}", &script.display().to_string())))
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k, v)))
                .env("HOME", runner.home.path())
                .env("GITHUB_WORKSPACE", runner.workspace.path())
                .env("GITHUB_OUTPUT", &output)
                .current_dir(runner.workspace.path())
                .stdout(fs::File::create(&stdout)?)
                .stderr(fs::File::create(&stderr)?)
                .spawn()?,
        );
        Ok(Self {
            child,
            output,
            stdout,
            stderr,
        })
    }

    fn captured(&self, status: ExitStatus) -> Result<Captured, Box<dyn Error>> {
        Ok(Captured {
            code: status.code(),
            stdout: fs::read_to_string(&self.stdout)?,
            stderr: fs::read_to_string(&self.stderr)?,
            github_output: fs::read_to_string(&self.output)?,
        })
    }
}

pub(super) fn verify(
    runner: &Sandbox,
    step: &RenderedCommand,
    env: &[(String, String)],
    artifact: &std::path::Path,
) -> Result<Captured, Box<dyn Error>> {
    let mut probe = Probe::start(runner, step, env)?;
    let started = Instant::now();
    let mut hash = stop_hash(probe.child.0.id(), artifact, started)?;
    let status = loop {
        if let Some(status) = probe.child.0.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(25) {
            return Err("owned verifier exceeded independent deadline".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let captured = probe.captured(status)?;
    assert!(
        captured
            .stdout
            .contains("download CAS hash deadline exceeded"),
        "{}",
        captured.stdout
    );
    let exists = Command::new("/bin/kill")
        .args(["-0", &hash.pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success();
    assert!(!exists, "timed out hasher was not reaped");
    hash.armed = false;
    Ok(captured)
}

pub(super) fn forbid_hash(
    runner: &Sandbox,
    step: &RenderedCommand,
    env: &[(String, String)],
    artifact: &Path,
) -> Result<Captured, Box<dyn Error>> {
    let started = Instant::now();
    let mut probe = Probe::start(runner, step, env)?;
    loop {
        if let Some(pid) = owned_hash(probe.child.0.id(), artifact)? {
            let hash = StoppedHash {
                pid,
                owner: probe.child.0.id(),
                artifact: artifact.to_owned(),
                armed: true,
            };
            hash.terminate()?;
            eprintln!("cas_oversize_observed_forbidden_owned_hash={pid}");
            return Err("oversized artifact content was read by owned hasher".into());
        }
        if let Some(status) = probe.child.0.try_wait()? {
            return probe.captured(status);
        }
        if started.elapsed() > Duration::from_secs(10) {
            return Err("oversize observer exceeded independent deadline".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn stop_hash(
    owner: u32,
    artifact: &std::path::Path,
    started: Instant,
) -> Result<StoppedHash, Box<dyn Error>> {
    while started.elapsed() < Duration::from_secs(5) {
        if let Some(pid) = owned_hash(owner, artifact)? {
            if !Command::new("/bin/kill")
                .args(["-STOP", &pid.to_string()])
                .status()?
                .success()
            {
                continue;
            }
            let hash = StoppedHash {
                pid,
                owner,
                artifact: artifact.to_owned(),
                armed: true,
            };
            let observed = Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()?;
            assert!(
                String::from_utf8(observed.stdout)?.contains('T'),
                "owned hasher did not stop"
            );
            eprintln!("cas_timeout_stopped_owned_hash={pid} verifier={owner}");
            return Ok(hash);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("did not observe owned hash subprocess".into())
}

fn owned_hash(owner: u32, artifact: &Path) -> Result<Option<u32>, Box<dyn Error>> {
    let listing = Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()?;
    if !listing.status.success() {
        return Err("owned process inspection failed".into());
    }
    let text = String::from_utf8(listing.stdout)?;
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    let parents: BTreeMap<u32, u32> = rows
        .iter()
        .filter_map(|row| Some((row.first()?.parse().ok()?, row.get(1)?.parse().ok()?)))
        .collect();
    for row in &rows {
        if row.last().copied() != artifact.to_str()
            || !row.iter().any(|word| {
                matches!(
                    *word,
                    "/usr/bin/shasum" | "/usr/bin/sha256sum" | "shasum" | "sha256sum"
                )
            })
        {
            continue;
        }
        let pid: u32 = row.first().ok_or("no hash PID")?.parse()?;
        if !descends(pid, owner, &parents) {
            continue;
        }
        return Ok(Some(pid));
    }
    Ok(None)
}

fn descends(mut pid: u32, owner: u32, parents: &BTreeMap<u32, u32>) -> bool {
    for _ in 0..32 {
        let Some(parent) = parents.get(&pid) else {
            return false;
        };
        if *parent == owner {
            return true;
        }
        if *parent == pid {
            return false;
        }
        pid = *parent;
    }
    false
}
