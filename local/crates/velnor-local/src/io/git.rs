// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use tokio::process::Command;

use crate::rest::{self, COMMIT_FORMAT, Commit, FileDiff, TreeEntry};

#[derive(Debug)]
pub(super) struct Git {
    pub(super) repo: PathBuf,
}

impl Git {
    pub(super) async fn run(&self, args: &[&str]) -> Option<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .arg("--literal-pathspecs")
            .args(["-c", "core.quotepath=false"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .await
            .ok()?;
        output.status.success().then_some(output.stdout)
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

    pub(super) async fn diff(&self, base: &str, head: &str) -> Option<Vec<FileDiff>> {
        let range = format!("{base}...{head}");
        let text = self
            .text(&[
                "diff",
                "-M",
                "--full-index",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--end-of-options",
                &range,
            ])
            .await?;
        let mut files = rest::parse_diff(&text);
        for file in &mut files {
            if file.sha.is_none() && file.status != "removed" {
                let spec = format!("{head}:{}", file.filename);
                file.sha = self
                    .text(&["rev-parse", "--verify", "--end-of-options", &spec])
                    .await
                    .map(|sha| sha.trim().to_owned());
            }
        }
        Some(files)
    }

    pub(super) async fn commits(&self, options: &[&str], revisions: &str) -> Option<Vec<Commit>> {
        let format = format!("--format={COMMIT_FORMAT}");
        let mut full = vec!["log", "--no-color", &format];
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
