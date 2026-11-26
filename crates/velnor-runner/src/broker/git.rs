// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rustix::io::Errno;
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};
use std::os::unix::process::CommandExt;
use tokio::io::AsyncReadExt;
use tokio::process::ChildStderr;
use tokio::runtime::{Handle, RuntimeFlavor};
use url::Url;

const GIT_ENV: [(&str, &str); 3] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

pub(crate) const TREE_PROBE: [&str; 3] = ["rev-list", "--objects", "--quiet"];

const NETWORK_ENV: [&str; 11] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "GIT_SSL_CAINFO",
];

pub(crate) fn isolate(command: &mut Command) -> &mut Command {
    command.env_clear();
    for name in ["PATH", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("HOME", "/dev/null").envs(GIT_ENV)
}

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

pub(super) fn fetch_env(url: &Url, token: Option<&str>) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = NETWORK_ENV
        .iter()
        .filter_map(|name| Some((OsString::from(name), std::env::var_os(name)?)))
        .collect();
    if let Some(token) = token.filter(|token| !token.is_empty())
        && url.scheme() == "https"
    {
        let credential = STANDARD.encode(format!("x-access-token:{token}"));
        env.extend([
            ("GIT_CONFIG_COUNT".into(), "1".into()),
            (
                "GIT_CONFIG_KEY_0".into(),
                format!("http.{}/.extraheader", url.origin().ascii_serialization()).into(),
            ),
            (
                "GIT_CONFIG_VALUE_0".into(),
                format!("AUTHORIZATION: basic {credential}").into(),
            ),
        ]);
    }
    env
}

struct Leader {
    child: Child,
    group: Pid,
}

fn wait_for_exit(leader: Pid) -> Result<bool, Errno> {
    loop {
        match waitid(
            WaitId::Pid(leader),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => {
                return Ok(status.is_some_and(|status| status.exit_status() == Some(0)));
            }
            Err(Errno::INTR) => {}
            Err(error) => return Err(error),
        }
    }
}

impl Leader {
    fn spawn(args: &[String], env: &[(OsString, OsString)]) -> Result<Self> {
        let mut command = Command::new("git");
        let child = isolate(&mut command)
            .args(args)
            .envs(env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .context("start git")?;
        let group = i32::try_from(child.id())
            .ok()
            .and_then(Pid::from_raw)
            .context("git has no process id")?;
        Ok(Self { child, group })
    }

    async fn finish(&mut self) -> Result<(bool, String)> {
        let stderr = self.child.stderr.take().context("git has no stderr")?;
        let mut stderr = ChildStderr::from_std(stderr).context("watch git stderr")?;
        let exit = tokio::task::spawn_blocking({
            let leader = self.group;
            move || wait_for_exit(leader)
        });
        let mut text = Vec::new();
        let (read, exit) = tokio::join!(stderr.read_to_end(&mut text), exit);
        let succeeded = exit
            .context("wait for the git exit task")?
            .context("wait for git")?;
        read.context("read git stderr")?;
        Ok((succeeded, String::from_utf8_lossy(&text).trim().to_owned()))
    }

    fn reap_group(&mut self) {
        let _ = self.child.wait();
        for _ in 0..200 {
            if test_kill_process_group(self.group).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Leader {
    fn drop(&mut self) {
        let _ = kill_process_group(self.group, Signal::KILL);
        match Handle::try_current().map(|handle| handle.runtime_flavor()) {
            Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(|| self.reap_group()),
            _ => self.reap_group(),
        }
    }
}

pub(super) async fn git(args: &[String], env: &[(OsString, OsString)]) -> Result<()> {
    let mut leader = Leader::spawn(args, env)?;
    let (succeeded, stderr) = leader.finish().await?;
    if !succeeded {
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

pub(super) async fn has_tree(git_dir: &Path, sha: &str) -> bool {
    let tree = format!("{sha}^{{tree}}");
    let probe = [TREE_PROBE.as_slice(), &[tree.as_str()]].concat();
    git(&git_args(Some(git_dir), &probe), &[]).await.is_ok()
}

#[cfg(test)]
mod tests;
