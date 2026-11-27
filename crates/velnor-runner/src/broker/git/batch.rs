// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::task::JoinHandle;

use super::{Leader, Pipes, git_args, line_per};

const BODY_LIMIT: u64 = 32 << 20;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Commit {
    Parents(Vec<String>),
    Damaged,
}

pub(crate) struct Batch {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: JoinHandle<String>,
    _leader: Leader,
}

fn object_id(word: &str) -> Option<&str> {
    let hex = word
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    (matches!(word.len(), 40 | 64) && hex).then_some(word)
}

fn parse_commit(body: &[u8]) -> Commit {
    let text = String::from_utf8_lossy(body);
    let mut lines = text.lines();
    let Some(tree) = lines.next().and_then(|line| line.strip_prefix("tree ")) else {
        return Commit::Damaged;
    };
    if object_id(tree).is_none() {
        return Commit::Damaged;
    }
    let mut parents = Vec::new();
    for line in lines {
        let Some(id) = line.strip_prefix("parent ") else {
            break;
        };
        match object_id(id) {
            Some(id) => parents.push(id.to_owned()),
            None => return Commit::Damaged,
        }
    }
    Commit::Parents(parents)
}

async fn answer(stdout: &mut BufReader<ChildStdout>, id: &str) -> Result<Commit> {
    let mut header = String::new();
    if stdout
        .read_line(&mut header)
        .await
        .context("read git cat-file")?
        == 0
    {
        bail!("git cat-file ended before it answered {id}");
    }
    let mut words = header.trim_end().split(' ');
    if words.next() != Some(id) {
        bail!("git cat-file answered {header:?} to {id}");
    }
    let kind = words.next().unwrap_or_default();
    if kind == "missing" {
        return Ok(Commit::Damaged);
    }
    let size: u64 = words
        .next()
        .and_then(|size| size.parse().ok())
        .with_context(|| format!("git cat-file answered {header:?} to {id}"))?;
    let kept = size.min(BODY_LIMIT);
    let mut body = Vec::new();
    (&mut *stdout)
        .take(kept)
        .read_to_end(&mut body)
        .await
        .context("read a commit from git cat-file")?;
    tokio::io::copy(
        &mut (&mut *stdout).take(size - kept),
        &mut tokio::io::sink(),
    )
    .await
    .context("skip a commit body from git cat-file")?;
    let mut newline = [0_u8; 1];
    stdout
        .read_exact(&mut newline)
        .await
        .context("read the end of a git cat-file entry")?;
    Ok(if kind == "commit" && size <= BODY_LIMIT {
        parse_commit(&body)
    } else {
        Commit::Damaged
    })
}

impl Batch {
    pub(crate) fn open(git_dir: &Path) -> Result<Self> {
        let args = git_args(Some(git_dir), &["cat-file", "--batch"]);
        let mut leader = Leader::spawn(&args, &[], Pipes::Capture { feed: true })?;
        let stdin = leader.child.stdin.take().context("git has no stdin")?;
        let stdout = leader.child.stdout.take().context("git has no stdout")?;
        let stderr = leader.child.stderr.take().context("git has no stderr")?;
        let mut stderr = ChildStderr::from_std(stderr).context("watch git stderr")?;
        Ok(Self {
            stdin: ChildStdin::from_std(stdin).context("feed git stdin")?,
            stdout: BufReader::new(ChildStdout::from_std(stdout).context("watch git stdout")?),
            stderr: tokio::spawn(async move {
                let mut text = Vec::new();
                let _ = stderr.read_to_end(&mut text).await;
                String::from_utf8_lossy(&text).trim().to_owned()
            }),
            _leader: leader,
        })
    }

    pub(crate) async fn commits(&mut self, ids: &[String]) -> Result<Vec<Commit>> {
        let input = line_per(ids.iter().map(String::as_str));
        let Self { stdin, stdout, .. } = self;
        let write = async {
            stdin
                .write_all(input.as_bytes())
                .await
                .context("feed git cat-file")?;
            stdin.flush().await.context("feed git cat-file")
        };
        let read = async {
            let mut commits = Vec::with_capacity(ids.len());
            for id in ids {
                commits.push(answer(stdout, id).await?);
            }
            Ok(commits)
        };
        tokio::try_join!(write, read).map(|((), commits)| commits)
    }

    pub(crate) async fn finish(self) -> String {
        drop(self.stdin);
        self.stderr.await.unwrap_or_default()
    }
}
