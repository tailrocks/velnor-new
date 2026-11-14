// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io;

use serde_json::json;
use tokio::fs;
use tokio::process::Command;

use super::Driver;
use crate::event::{Changes, NO_COMMIT};
use crate::scenario::{next_patch, with_path};

impl Driver<'_> {
    pub(super) async fn changes(
        &self,
        branch: &str,
        before: &str,
        after: &str,
    ) -> io::Result<Vec<Changes>> {
        let others = format!("--exclude={branch}");
        let range = format!("{before}..{after}");
        let listed = if before == NO_COMMIT {
            let args = [
                "rev-list",
                "--reverse",
                after,
                "--not",
                &others,
                "--branches",
            ];
            self.git(&args).await?
        } else {
            self.git(&["rev-list", "--reverse", &range]).await?
        };
        let mut commits = Vec::new();
        for id in listed.lines() {
            let args = [
                "diff-tree",
                "-z",
                "--no-commit-id",
                "--name-status",
                "--no-renames",
                "-r",
                "--root",
                id,
            ];
            commits.push(Changes::parse(id, &self.git(&args).await?));
        }
        Ok(commits)
    }

    pub(super) async fn update_lock(
        &self,
        branch: &str,
        package: &str,
        workspace: &str,
    ) -> io::Result<()> {
        self.on_branch(branch).await?;
        let root = self.repo.join(workspace);
        let manifest = fs::canonicalize(root.join("Cargo.toml")).await?;
        let lock = root.join("Cargo.lock");
        let lock_inside = match fs::canonicalize(&lock).await {
            Ok(real) => real.starts_with(fs::canonicalize(&self.repo).await?),
            Err(_) => fs::symlink_metadata(&lock).await.is_err(),
        };
        if !manifest.starts_with(fs::canonicalize(&self.repo).await?) || !lock_inside {
            return Err(io::Error::other(
                "Cargo.toml or Cargo.lock leaves the copy of the checkout",
            ));
        }
        let current = fs::read_to_string(&lock).await?;
        let (spec, precise) = next_patch(&current, package).ok_or_else(|| {
            io::Error::other(format!(
                "Cargo.lock has no plain x.y.z version of {package}"
            ))
        })?;
        let update = [
            "update",
            "-p",
            &spec,
            "--precise",
            &precise,
            "--manifest-path",
        ];
        let updated = Command::new("cargo")
            .args(update)
            .arg(&manifest)
            .current_dir(&root)
            .env(
                "MISE_IGNORED_CONFIG_PATHS",
                with_path(
                    std::env::var("MISE_IGNORED_CONFIG_PATHS").ok().as_deref(),
                    &self.repo.to_string_lossy(),
                ),
            )
            .kill_on_drop(true)
            .output()
            .await?;
        if !updated.status.success() {
            let error = String::from_utf8_lossy(&updated.stderr);
            return Err(io::Error::other(format!("cargo update: {}", error.trim())));
        }
        let message = format!("Update {package} to {precise}");
        self.git(&["add", "-A"]).await?;
        self.git(&["commit", "-q", "-m", &message]).await?;
        Ok(())
    }

    pub(super) async fn schedule(&mut self, cron: &str) -> io::Result<()> {
        let branch = self.args.environment.default_branch.clone();
        self.git(&["switch", "-q", &branch]).await?;
        let sha = self.git(&["rev-parse", "HEAD"]).await?;
        let workflow = self.args.workflow.as_deref().unwrap_or_default();
        let payload = self.repository().schedule(cron, workflow);
        self.send("schedule", &payload, &format!("refs/heads/{branch}"), &sha)
            .await
    }

    pub(super) async fn dispatch(
        &mut self,
        branch: &str,
        inputs: &BTreeMap<String, serde_json::Value>,
    ) -> io::Result<()> {
        self.git(&["switch", "-q", branch]).await?;
        let sha = self.git(&["rev-parse", "HEAD"]).await?;
        let workflow = self.args.workflow.as_deref().ok_or_else(|| {
            io::Error::other(
                "a workflow_dispatch step needs --workflow, as GitHub dispatches one workflow",
            )
        })?;
        let inputs = json!(inputs);
        let payload = self
            .repository()
            .workflow_dispatch(branch, &inputs, workflow);
        self.send(
            "workflow_dispatch",
            &payload,
            &format!("refs/heads/{branch}"),
            &sha,
        )
        .await
    }
}
