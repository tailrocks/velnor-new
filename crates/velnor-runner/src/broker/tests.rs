// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

use super::cache::{CacheOwner, KEY, MARKER, VERSION, entry_dir, sha256_hex};
use super::facts::TrustedFacts;
use super::proto::Response;
use super::{Broker, CacheUpload};

pub(super) const TAR: &[u8] = b"archive";
pub(super) const FORK: &str =
    r#"{"kind": "pull_request", "number": 7, "base_branch": "main", "head": "fork"}"#;
pub(super) const BRANCH: &str = r#"{"kind": "branch", "branch": "main"}"#;

pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("velnor-broker-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

fn unlock(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        for item in fs::read_dir(path).into_iter().flatten().flatten() {
            unlock(&item.path());
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        unlock(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Fixture {
    pub(super) scratch: Scratch,
    pub(super) broker: Arc<Broker>,
    pub(super) facts: TrustedFacts,
}

fn paths_below(directory: &Path, found: &mut Vec<PathBuf>) {
    for item in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = item.path();
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            paths_below(&path, found);
        }
        found.push(path);
    }
}

pub(super) fn facts_named(name: &str, event: &str) -> Result<TrustedFacts> {
    let body = format!(
        r#"{{"repository_id": 4242, "repository": "tailrocks/velnor-new",
            "server_url": "https://github.com", "default_branch": "main",
            "sha": "0123456789abcdef0123456789abcdef01234567", "run_id": 1,
            "run_attempt": 1, "event_name": "{name}", "event": {event}}}"#
    );
    Ok(TrustedFacts::parse(body.as_bytes())?)
}

pub(super) fn facts_of(event: &str) -> Result<TrustedFacts> {
    let name = if event.contains("pull_request") {
        "pull_request"
    } else {
        "push"
    };
    facts_named(name, event)
}

impl Fixture {
    pub(super) fn new() -> Result<Self> {
        Self::with(BRANCH, |broker| broker)
    }

    pub(super) fn with(event: &str, configure: impl FnOnce(Broker) -> Broker) -> Result<Self> {
        Self::from_facts(facts_of(event)?, configure)
    }

    pub(super) fn from_facts(
        facts: TrustedFacts,
        configure: impl FnOnce(Broker) -> Broker,
    ) -> Result<Self> {
        let scratch = Scratch::new()?;
        let broker = Arc::new(configure(Broker::new(
            facts.clone(),
            scratch.0.join("store"),
        )));
        Ok(Self {
            scratch,
            broker,
            facts,
        })
    }

    pub(super) fn root(&self) -> PathBuf {
        self.scratch.0.join("store")
    }

    pub(super) fn incoming(&self) -> PathBuf {
        self.broker.layout.incoming()
    }

    pub(super) fn upload_dir(&self) -> Result<PathBuf> {
        let layout = &self.broker.layout;
        let name = layout.next_name();
        let _ = layout.create_incoming(&name)?;
        Ok(layout.incoming().join(name))
    }

    pub(super) fn incoming_names(&self) -> Vec<String> {
        fs::read_dir(self.incoming())
            .into_iter()
            .flatten()
            .flatten()
            .map(|item| item.file_name().to_string_lossy().into_owned())
            .collect()
    }

    pub(super) fn paths_in_the_store(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        paths_below(&self.root().join("cache").join("v1"), &mut found);
        found
    }

    pub(super) fn assert_nothing_published(&self) {
        let published: Vec<_> = self
            .paths_in_the_store()
            .into_iter()
            .filter(|path| path.file_name().is_some_and(|name| name == MARKER))
            .collect();
        assert_eq!(published, Vec::<PathBuf>::new());
    }

    pub(super) fn assert_no_leftover(&self) {
        let root = self.root().join("cache").join("v1");
        let leftovers: Vec<_> = self
            .paths_in_the_store()
            .into_iter()
            .filter(|path| {
                path.strip_prefix(&root)
                    .is_ok_and(|rest| rest.components().count() > 2)
            })
            .collect();
        assert_eq!(leftovers, Vec::<PathBuf>::new());
        assert_eq!(self.incoming_names(), Vec::<String>::new());
    }

    pub(super) fn entry(&self, key: &str) -> Result<PathBuf> {
        Ok(entry_dir(
            &self.scratch.0.join("store"),
            &CacheOwner::of(&self.facts),
            &self
                .facts
                .cache_write_scope()
                .map_err(|denied| anyhow::anyhow!("{denied}"))?,
            &version(),
            key,
        ))
    }

    pub(super) fn victim(&self) -> Result<PathBuf> {
        let victim = self.scratch.0.join("victim");
        fs::write(&victim, "untouched")?;
        Ok(victim)
    }

    pub(super) async fn upload(&self, key: &str, bytes: &[u8]) -> Response {
        upload(&self.broker, key, bytes).await
    }
}

pub(super) fn version() -> String {
    sha256_hex("paths")
}

pub(super) async fn write_all(upload: &mut CacheUpload<'_>, bytes: Vec<u8>) -> Result<u64> {
    let mut writer = upload.take_writer().context("the pipe was taken")?;
    tokio::task::spawn_blocking(move || {
        writer.write_all(&bytes)?;
        Ok(bytes.len() as u64)
    })
    .await?
}

pub(super) async fn upload(broker: &Broker, key: &str, bytes: &[u8]) -> Response {
    let mut upload = match broker.begin_cache(key, &version()).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    match write_all(&mut upload, bytes.to_vec()).await {
        Ok(count) => upload.seal(count).await,
        Err(error) => Response::Failed {
            message: format!("{error:#}"),
        },
    }
}

pub(super) async fn begin<'a>(broker: &'a Broker, key: &str) -> Result<CacheUpload<'a>> {
    broker
        .begin_cache(key, &version())
        .await
        .map_err(|response| anyhow::anyhow!("{response:?}"))
}

pub(super) fn untouched(victim: &Path) -> Result<()> {
    assert_eq!(fs::read_to_string(victim)?, "untouched");
    Ok(())
}

pub(super) fn published(response: &Response) -> bool {
    matches!(response, Response::Published { .. })
}

pub(super) fn failed(response: &Response, message: &str) {
    assert!(
        matches!(response, Response::Failed { message: text } if text.contains(message)),
        "{message}: {response:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_planted_in_the_entry_during_publish_is_never_written_through() -> Result<()> {
    let fixture = Fixture::new()?;
    let victim = fixture.victim()?;
    for round in 0..20 {
        let key = format!("k-{round}");
        let entry = fixture.entry(&key)?;
        let stop = Arc::new(AtomicBool::new(false));
        let planter = {
            let (stop, victim, entry) = (stop.clone(), victim.clone(), entry.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    for name in [KEY, VERSION, MARKER] {
                        let _ = symlink(&victim, entry.join(name));
                    }
                    std::thread::yield_now();
                }
            })
        };
        let response = fixture.upload(&key, TAR).await;
        stop.store(true, Ordering::Relaxed);
        assert!(planter.join().is_ok());
        untouched(&victim)?;
        match response {
            Response::Published { .. } => {
                for name in [KEY, VERSION, MARKER] {
                    assert!(fs::symlink_metadata(entry.join(name))?.is_file());
                }
            }
            Response::Failed { .. } => {}
            other => bail!("unexpected answer {other:?}"),
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishers_of_one_key_publish_exactly_once() -> Result<()> {
    const PUBLISHERS: usize = 16;
    let fixture = Fixture::new()?;
    let mut tasks = Vec::new();
    for _ in 0..PUBLISHERS {
        let broker = fixture.broker.clone();
        tasks.push(tokio::spawn(
            async move { upload(&broker, "k-1", TAR).await },
        ));
    }
    let mut answers = Vec::new();
    for task in tasks {
        answers.push(task.await?);
    }
    let published = answers.iter().filter(|answer| published(answer)).count();
    let existing = answers
        .iter()
        .filter(|answer| matches!(answer, Response::AlreadyExists))
        .count();
    assert_eq!((published, existing), (1, PUBLISHERS - 1), "{answers:?}");
    assert_eq!(fs::read(fixture.entry("k-1")?.join("data.tar"))?, TAR);
    assert_eq!(fixture.incoming_names(), Vec::<String>::new());
    Ok(())
}
