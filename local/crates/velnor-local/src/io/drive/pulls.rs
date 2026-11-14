// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use serde_json::{Value, json};
use tokio::fs;

use super::Driver;
use crate::event::{Commit, MergeGroup, PullRequest};
use crate::io::env::request;
use crate::io::now;
use crate::rest::rfc3339;

#[derive(Debug)]
pub(super) struct Pull {
    number: u64,
    base: String,
    head: String,
    base_sha: String,
    head_sha: String,
    open: bool,
    merged: bool,
    fork: bool,
}

impl Driver<'_> {
    fn pull(&self, head: &str) -> io::Result<usize> {
        self.pulls
            .iter()
            .position(|pull| pull.open && pull.head == head)
            .ok_or_else(|| io::Error::other(format!("no open pull request has the head {head}")))
    }

    async fn merge_commit(&self, base_sha: &str, head_sha: &str) -> io::Result<String> {
        self.git(&["switch", "-q", "--detach", base_sha]).await?;
        self.git(&["merge", "-q", "--no-ff", "--no-edit", head_sha])
            .await?;
        self.git(&["rev-parse", "HEAD"]).await
    }

    async fn announce(&mut self, at: usize, action: &str, before: Option<&str>) -> io::Result<()> {
        let pull = &self.pulls[at];
        let range = format!("{}...{}", pull.base_sha, pull.head_sha);
        let changed = self.git(&["diff", "--name-only", &range]).await?;
        let refs = format!("refs/pull/{}", pull.number);
        let (git_ref, sha) = if pull.merged {
            let base = format!("refs/heads/{}", pull.base);
            let sha = self.git(&["rev-parse", &base]).await?;
            (base, sha)
        } else if action == "closed" {
            let merge = self.git(&["rev-parse", &format!("{refs}/merge")]).await?;
            (format!("{refs}/merge"), merge)
        } else {
            let merge = self.merge_commit(&pull.base_sha, &pull.head_sha).await?;
            let head = &self.pulls[at].head_sha;
            self.git(&["update-ref", &format!("{refs}/merge"), &merge])
                .await?;
            self.git(&["update-ref", &format!("{refs}/head"), head])
                .await?;
            (format!("{refs}/merge"), merge)
        };
        self.write_pulls().await?;
        let pull = &self.pulls[at];
        let closed_at = (action == "closed").then(|| rfc3339(now()));
        let payload = self.repository().pull_request(&PullRequest {
            action,
            number: pull.number,
            base: (&pull.base, &pull.base_sha),
            head: (&pull.head, &pull.head_sha),
            before,
            merged: pull.merged,
            merge_commit: &sha,
            fork: pull.fork,
            closed_at: closed_at.as_deref(),
            changed_files: changed.lines().count(),
        });
        self.send("pull_request", &payload, &git_ref, &sha).await
    }

    pub(super) async fn open(&mut self, head: &str, base: &str, fork: bool) -> io::Result<()> {
        let number = self.pulls.len() as u64 + 1;
        let registration = json!({"base_branch": base}).to_string();
        let path = format!("/_velnor/pull-requests/{number}");
        request(&self.addresses.cache, "PUT", &path, &registration)
            .await
            .ok_or_else(|| io::Error::other(format!("cannot register pull request {number}")))?;
        self.pulls.push(Pull {
            number,
            base: base.to_owned(),
            head: head.to_owned(),
            base_sha: self.git(&["rev-parse", base]).await?,
            head_sha: self.git(&["rev-parse", head]).await?,
            open: true,
            merged: false,
            fork,
        });
        let head_sha = self.pulls[self.pulls.len() - 1].head_sha.clone();
        self.pushed.insert(head.to_owned(), head_sha);
        self.announce(self.pulls.len() - 1, "opened", None).await
    }

    async fn write_pulls(&self) -> io::Result<()> {
        let listed: Vec<Value> = self
            .pulls
            .iter()
            .map(|pull| {
                json!({
                    "number": pull.number,
                    "state": if pull.open { "open" } else { "closed" },
                    "base": {"ref": pull.base, "sha": pull.base_sha},
                    "head": {"ref": pull.head, "sha": pull.head_sha},
                })
            })
            .collect();
        let pulls = Value::Array(listed).to_string();
        fs::write(self.env.dir.join("api/pulls.json"), pulls).await
    }

    pub(super) async fn retarget(
        &mut self,
        branch: &str,
        after: &str,
    ) -> io::Result<Vec<(usize, String)>> {
        let mut moved = Vec::new();
        let mut rebased = false;
        for at in 0..self.pulls.len() {
            if self.pulls[at].open && self.pulls[at].head == branch {
                let before = std::mem::replace(&mut self.pulls[at].head_sha, after.to_owned());
                self.pulls[at].base_sha = self.git(&["rev-parse", &self.pulls[at].base]).await?;
                moved.push((at, before));
            } else if self.pulls[at].open && self.pulls[at].base == branch {
                after.clone_into(&mut self.pulls[at].base_sha);
                let (number, head) = (self.pulls[at].number, self.pulls[at].head_sha.clone());
                if let Ok(merge) = self.merge_commit(after, &head).await {
                    let merge_ref = format!("refs/pull/{number}/merge");
                    self.git(&["update-ref", &merge_ref, &merge]).await?;
                } else {
                    let _ = self.git(&["merge", "--abort"]).await;
                }
                rebased = true;
            }
        }
        if !moved.is_empty() || rebased {
            self.write_pulls().await?;
        }
        Ok(moved)
    }

    pub(super) async fn synchronize(&mut self, moved: Vec<(usize, String)>) -> io::Result<()> {
        for (at, before) in moved {
            let (base_sha, number) = (self.pulls[at].base_sha.clone(), self.pulls[at].number);
            let after = self.pulls[at].head_sha.clone();
            if self.merge_commit(&base_sha, &after).await.is_err() {
                let _ = self.git(&["merge", "--abort"]).await;
                println!(
                    "velnor-local {}: pull request {number} has a merge conflict, so no pull_request event",
                    self.tag
                );
                continue;
            }
            self.announce(at, "synchronize", Some(&before)).await?;
        }
        Ok(())
    }

    pub(super) async fn close(&mut self, head: &str) -> io::Result<()> {
        let at = self.pull(head)?;
        self.pulls[at].open = false;
        self.announce(at, "closed", None).await
    }

    pub(super) async fn merge(&mut self, head: &str) -> io::Result<()> {
        let at = self.pull(head)?;
        let (base, number) = (self.pulls[at].base.clone(), self.pulls[at].number);
        self.git(&["switch", "-q", &base]).await?;
        let tip = self.git(&["rev-parse", "HEAD"]).await?;
        self.pushed.insert(base.clone(), tip);
        self.git(&["merge", "-q", "--squash", &self.pulls[at].head_sha])
            .await?;
        let message = format!("Merge {head} (#{number})");
        self.git(&["commit", "-q", "-m", &message]).await?;
        self.pulls[at].open = false;
        self.pulls[at].merged = true;
        self.announce(at, "closed", None).await?;
        self.push(&base).await
    }

    pub(super) async fn merge_group(&mut self, head: &str) -> io::Result<()> {
        let at = self.pull(head)?;
        let pull = &self.pulls[at];
        let base_sha = self.git(&["rev-parse", &pull.base]).await?;
        let queue = format!(
            "gh-readonly-queue/{}/pr-{}-{base_sha}",
            pull.base, pull.number
        );
        let base = pull.base.clone();
        let merge = self
            .merge_commit(&base_sha, &self.pulls[at].head_sha)
            .await?;
        self.git(&["update-ref", &format!("refs/heads/{queue}"), &merge])
            .await?;
        let log = self
            .git(&[
                "log",
                "-1",
                "--format=%T%x00%cI%x00%an%x00%ae%x00%cn%x00%ce%x00%B",
                &merge,
            ])
            .await?;
        let commit = Commit::parse(&log)
            .ok_or_else(|| io::Error::other(format!("cannot read commit {merge}")))?;
        let payload = self.repository().merge_group(&MergeGroup {
            base: (&base, &base_sha),
            head: (&queue, &merge),
            commit: &commit,
        });
        self.send(
            "merge_group",
            &payload,
            &format!("refs/heads/{queue}"),
            &merge,
        )
        .await
    }
}
