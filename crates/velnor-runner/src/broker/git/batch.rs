// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

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

enum Answer {
    Commit(Commit),
    Ended,
}

struct Session {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: JoinHandle<String>,
    _leader: Leader,
}

pub(crate) struct Batch {
    git_dir: PathBuf,
    session: Session,
    noted: String,
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

async fn answer(stdout: &mut BufReader<ChildStdout>, id: &str) -> Result<Answer> {
    let mut header = String::new();
    let read = stdout
        .read_line(&mut header)
        .await
        .context("read git cat-file")?;
    if read == 0 || !header.ends_with('\n') {
        return Ok(Answer::Ended);
    }
    let mut words = header.trim_end().split(' ');
    if words.next() != Some(id) {
        bail!("git cat-file answered {header:?} to {id}");
    }
    let kind = words.next().unwrap_or_default();
    if kind == "missing" {
        return Ok(Answer::Commit(Commit::Damaged));
    }
    let size: u64 = words
        .next()
        .and_then(|size| size.parse().ok())
        .with_context(|| format!("git cat-file answered {header:?} to {id}"))?;
    let kept = size.min(BODY_LIMIT);
    let mut body = Vec::new();
    let copied = tokio::io::copy(&mut (&mut *stdout).take(kept), &mut body)
        .await
        .context("read a commit from git cat-file")?;
    let skipped = tokio::io::copy(
        &mut (&mut *stdout).take(size - kept),
        &mut tokio::io::sink(),
    )
    .await
    .context("skip a commit body from git cat-file")?;
    let mut newline = [0_u8; 1];
    match stdout.read_exact(&mut newline).await {
        Ok(_) if copied == kept && skipped == size - kept => {}
        Ok(_) => return Ok(Answer::Ended),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Ok(Answer::Ended);
        }
        Err(error) => return Err(error).context("read the end of a git cat-file entry"),
    }
    Ok(Answer::Commit(if kind == "commit" && size <= BODY_LIMIT {
        parse_commit(&body)
    } else {
        Commit::Damaged
    }))
}

impl Session {
    fn open(git_dir: &Path) -> Result<Self> {
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

    async fn ask(&mut self, ids: &[String], answered: &mut Vec<Commit>) -> Result<()> {
        let input = line_per(ids.iter().map(String::as_str));
        let Self { stdin, stdout, .. } = self;
        let write = async {
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.flush().await;
        };
        let read = async {
            for id in ids {
                match answer(stdout, id).await? {
                    Answer::Commit(commit) => answered.push(commit),
                    Answer::Ended => break,
                }
            }
            Ok::<(), anyhow::Error>(())
        };
        let ((), read) = tokio::join!(write, read);
        read
    }

    async fn finish(self) -> String {
        drop(self.stdin);
        self.stderr.await.unwrap_or_default()
    }
}

impl Batch {
    pub(crate) fn open(git_dir: &Path) -> Result<Self> {
        Ok(Self {
            git_dir: git_dir.to_owned(),
            session: Session::open(git_dir)?,
            noted: String::new(),
        })
    }

    pub(crate) async fn commits(&mut self, ids: &[String]) -> Result<Vec<Commit>> {
        let mut commits = Vec::with_capacity(ids.len());
        while commits.len() < ids.len() {
            self.session
                .ask(&ids[commits.len()..], &mut commits)
                .await?;
            if commits.len() < ids.len() {
                commits.push(Commit::Damaged);
                let next = Session::open(&self.git_dir)?;
                let ended = std::mem::replace(&mut self.session, next);
                self.noted.extend([ended.finish().await.as_str(), "\n"]);
            }
        }
        Ok(commits)
    }

    pub(crate) async fn finish(self) -> String {
        let last = self.session.finish().await;
        format!("{}{last}", self.noted)
    }
}
