// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{DEFAULT_SNAPSHOT, Scene, fail};

const RUN_DEADLINE: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(25);

fn drain<R: std::io::Read + Send + 'static>(
    mut source: R,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

impl Scene {
    pub(crate) fn run_script(
        &self,
        script: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        self.run_script_unset(script, env, &[], fsize_blocks)
    }

    pub(crate) fn run_runner_script(
        &self,
        script: &str,
        env: &[(&str, &str)],
    ) -> Result<String, Box<dyn Error>> {
        let path = self.scratch.join("runner step's script.sh");
        std::fs::write(&path, script)?;
        let path_env = format!("{}:{}", self.bin.display(), std::env::var("PATH")?);
        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-e", "-o", "pipefail"])
            .arg(path)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.base_env(&mut command, &path_env);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn()?;
        Self::collect(&mut child)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn spawn_runner_script(
        &self,
        script: &str,
        env: &[(&str, &str)],
    ) -> Result<std::process::Child, Box<dyn Error>> {
        let path = self.scratch.join("runner step's script.sh");
        std::fs::write(&path, script)?;
        let path_env = format!("{}:{}", self.bin.display(), std::env::var("PATH")?);
        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-e", "-o", "pipefail"])
            .arg(path)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.base_env(&mut command, &path_env);
        for (name, value) in env {
            command.env(name, value);
        }
        Ok(command.spawn()?)
    }

    pub(crate) fn run_script_unset(
        &self,
        script: &str,
        env: &[(&str, &str)],
        unset: &[&str],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH")?);
        let mut command = Command::new("sh");
        match fsize_blocks {
            Some(blocks) => command
                .arg("-c")
                .arg("ulimit -f \"$1\"; exec sh -c \"$2\"")
                .arg("sh")
                .arg(blocks.to_string())
                .arg(script),
            None => command.arg("-c").arg(script),
        };
        self.base_env(&mut command, &path);
        for (name, value) in env {
            command.env(name, value);
        }
        for name in unset {
            command.env_remove(name);
        }
        let mut child = command
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Self::collect(&mut child)
    }

    fn base_env(&self, command: &mut Command, path: &str) {
        command
            .current_dir(&self.workspace)
            .env("PATH", path)
            .env("RUNNER_TEMP", &self.runner)
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("GITHUB_OUTPUT", &self.github_output)
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_REF_PROTECTED", "true")
            .env("VELNOR_DEFAULT_REF", "refs/heads/main")
            .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
            .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
            .env("VELNOR_MBX_BUNDLE_ROOT", ".")
            .env("VELNOR_STATE_SNAPSHOT", DEFAULT_SNAPSHOT)
            .env("MBX_SHARE_OUT_DIR", "1")
            .env("MBX_BUILD_SCRIPT_EXECUTION", "0")
            .env("MBX_CC", "0")
            .env("SOURCE_DATE_EPOCH", "0")
            .env("ImageOS", "ubuntu26")
            .env("MBX_CACHE_EXPORT_GROUP", "velnor")
            .env("MBX_CACHE_DIR", self.store());
    }

    fn collect(child: &mut std::process::Child) -> Result<String, Box<dyn Error>> {
        let stdout = drain(
            child
                .stdout
                .take()
                .ok_or_else(|| fail("stdout unavailable"))?,
        );
        let stderr = drain(
            child
                .stderr
                .take()
                .ok_or_else(|| fail("stderr unavailable"))?,
        );
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() > RUN_DEADLINE {
                Command::new("kill")
                    .arg("-KILL")
                    .arg("--")
                    .arg(format!("-{}", child.id()))
                    .status()?;
                child.wait()?;
                break None;
            }
            std::thread::sleep(POLL);
        };
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&stdout.join().map_err(|_| fail("stdout reader died"))??),
            String::from_utf8_lossy(&stderr.join().map_err(|_| fail("stderr reader died"))??)
        );
        match status {
            Some(status) if status.success() => Ok(text),
            Some(_) => Err(fail(&format!("script failed the step: {text}"))),
            None => Err(fail(&format!("script hung past the deadline: {text}"))),
        }
    }
}
