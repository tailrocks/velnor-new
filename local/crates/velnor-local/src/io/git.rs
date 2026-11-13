// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::rest::{self, COMMIT_FORMAT, Commit, FileDiff, TreeEntry};

#[derive(Debug)]
pub(super) struct Git {
    pub(super) repo: PathBuf,
    pub(super) diff_limit_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Missing,
    TooLarge,
}

impl Git {
    pub(super) async fn run(&self, args: &[&str]) -> Option<Vec<u8>> {
        self.capped(args, usize::MAX).await.ok()
    }

    async fn capped(&self, args: &[&str], limit: usize) -> Result<Vec<u8>, Failure> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .arg("--literal-pathspecs")
            .args(["-c", "core.quotepath=false"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Failure::Missing)?;
        let mut bytes = Vec::new();
        let stdout = child.stdout.take().ok_or(Failure::Missing)?;
        let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
        stdout
            .take(cap)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Failure::Missing)?;
        if bytes.len() > limit {
            return Err(Failure::TooLarge);
        }
        let status = child.wait().await.map_err(|_| Failure::Missing)?;
        status.success().then_some(bytes).ok_or(Failure::Missing)
    }

    pub(super) async fn text(&self, args: &[&str]) -> Option<String> {
        self.run(args)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    pub(super) async fn resolve(&self, reference: &str) -> Option<String> {
        if reference.is_empty() || reference.starts_with('-') {
            return None;
        }
        let spec = format!("{reference}^{{commit}}");
        let sha = self
            .text(&[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ])
            .await?;
        Some(sha.trim().to_owned())
    }

    pub(super) async fn diff(&self, base: &str, head: &str) -> Result<Vec<FileDiff>, Failure> {
        let range = format!("{base}...{head}");
        let bytes = self
            .capped(
                &[
                    "diff",
                    "-M",
                    "--full-index",
                    "--no-color",
                    "--no-ext-diff",
                    "--src-prefix=a/",
                    "--dst-prefix=b/",
                    "--end-of-options",
                    &range,
                ],
                self.diff_limit_bytes,
            )
            .await?;
        let mut files = rest::parse_diff(&String::from_utf8_lossy(&bytes));
        for file in &mut files {
            if file.sha.is_none() && file.status != "removed" {
                let spec = format!("{head}:{}", file.filename);
                file.sha = self
                    .text(&["rev-parse", "--verify", "--end-of-options", &spec])
                    .await
                    .map(|sha| sha.trim().to_owned());
            }
        }
        Ok(files)
    }

    pub(super) async fn commits(&self, options: &[&str], revisions: &str) -> Option<Vec<Commit>> {
        let format = format!("--format={COMMIT_FORMAT}");
        let mut full = vec!["log", "-z", "--no-color", &format];
        full.extend_from_slice(options);
        full.extend_from_slice(&["--end-of-options", revisions]);
        let text = self.text(&full).await?;
        Some(rest::parse_commits(&text))
    }

    pub(super) async fn tree(&self, commit: &str, path: &str) -> Option<Vec<TreeEntry>> {
        let text = self
            .text(&[
                "ls-tree",
                "-z",
                "-l",
                "--end-of-options",
                commit,
                "--",
                path,
            ])
            .await?;
        Some(rest::parse_tree(&text))
    }
}
