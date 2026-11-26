// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::runtime::{Handle, RuntimeFlavor};
use url::Url;

pub(crate) const GIT_ENV: [(&str, &str); 3] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

pub(crate) fn git_args(git_dir: Option<&Path>, args: &[&str]) -> Vec<String> {
    let mut all = vec![
        "-c".to_owned(),
        "core.hooksPath=/dev/null".to_owned(),
        "--no-replace-objects".to_owned(),
    ];
    if let Some(dir) = git_dir {
        all.push(format!("--git-dir={}", dir.display()));
    }
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    all
}

pub(super) fn auth_env(url: &Url, token: Option<&str>) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(token) = token.filter(|token| !token.is_empty())
        && url.scheme() == "https"
    {
        let credential = STANDARD.encode(format!("x-access-token:{token}"));
        env.extend([
            ("GIT_CONFIG_COUNT".to_owned(), "1".to_owned()),
            (
                "GIT_CONFIG_KEY_0".to_owned(),
                format!("http.{}/.extraheader", url.origin().ascii_serialization()),
            ),
            (
                "GIT_CONFIG_VALUE_0".to_owned(),
                format!("AUTHORIZATION: basic {credential}"),
            ),
        ]);
    }
    env
}

struct Leader {
    child: Child,
    group: Pid,
    reaped: bool,
}

impl Leader {
    fn spawn(args: &[String], env: &[(String, String)]) -> Result<Self> {
        let child = Command::new("git")
            .args(args)
            .envs(GIT_ENV)
            .env("HOME", "/dev/null")
            .envs(env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .context("start git")?;
        let group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(Pid::from_raw)
            .context("git has no process id")?;
        Ok(Self {
            child,
            group,
            reaped: false,
        })
    }

    async fn finish(&mut self) -> Result<(ExitStatus, String)> {
        let mut stderr = self.child.stderr.take().context("git has no stderr")?;
        let mut text = Vec::new();
        let reaped = &mut self.reaped;
        let child = &mut self.child;
        let wait = async {
            let status = child.wait().await;
            *reaped = true;
            status
        };
        let (read, status) = tokio::join!(stderr.read_to_end(&mut text), wait);
        let status = status.context("wait for git")?;
        read.context("read git stderr")?;
        Ok((status, String::from_utf8_lossy(&text).trim().to_owned()))
    }

    fn reap_group(&mut self) {
        for _ in 0..200 {
            let reaped = !matches!(self.child.try_wait(), Ok(None));
            if reaped && test_kill_process_group(self.group).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Leader {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        let _ = kill_process_group(self.group, Signal::KILL);
        match Handle::try_current().map(|handle| handle.runtime_flavor()) {
            Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(|| self.reap_group()),
            _ => self.reap_group(),
        }
    }
}

pub(super) async fn git(args: &[String], env: &[(String, String)]) -> Result<()> {
    let mut leader = Leader::spawn(args, env)?;
    let (status, stderr) = leader.finish().await?;
    if !status.success() {
        bail!("git {}: {stderr}", args.join(" "));
    }
    Ok(())
}

pub(super) async fn has_commit(git_dir: &Path, sha: &str) -> bool {
    let object = format!("{sha}^{{commit}}");
    git(&git_args(Some(git_dir), &["cat-file", "-e", &object]), &[])
        .await
        .is_ok()
}

#[cfg(test)]
mod tests;
