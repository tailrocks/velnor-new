// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{PipeReader, PipeWriter, Read as _, Write as _};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail, ensure};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use tokio::task::JoinHandle;

use super::cache::CacheOwner;
use super::facts::TrustedFacts;
use super::incoming::Upload;
use super::layout::Layout;
use super::proto::Response;
use super::publish::{Target, publish};

const CHUNK: usize = 1 << 20;
const TICK: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 20_000_000,
};
const NOW: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 0,
};

#[derive(Debug, thiserror::Error)]
#[error("the cache upload was aborted")]
struct Aborted;

#[derive(Default)]
struct Signal {
    aborted: AtomicBool,
    sealed: OnceLock<u64>,
}

struct Abandoned(Arc<AtomicBool>);

impl Drop for Abandoned {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct Intake {
    data: File,
    bytes: u64,
}

pub(crate) struct Session {
    layout: Layout,
    target: Target,
    upload: Arc<Upload>,
    signal: Arc<Signal>,
    intake: Option<JoinHandle<Result<Intake>>>,
    armed: bool,
}

fn failed(error: impl std::fmt::Display) -> Response {
    Response::Failed {
        message: format!("{error:#}"),
    }
}

fn over_limit(size: u64) -> anyhow::Error {
    anyhow::anyhow!(
        "Cache size of ~{} MB ({size} B) is over the 10GB limit, not saving cache.",
        (size + (1 << 19)) >> 20
    )
}

#[cfg(target_os = "linux")]
fn widen(pipe: &PipeWriter) {
    let _ = rustix::pipe::fcntl_setpipe_size(pipe, 1 << 20);
}

#[cfg(not(target_os = "linux"))]
fn widen(_: &PipeWriter) {}

fn readable(reader: &PipeReader, wait: &Timespec) -> Result<bool> {
    let mut fds = [PollFd::new(reader, PollFlags::IN)];
    match poll(&mut fds, Some(wait)) {
        Ok(ready) => Ok(ready > 0),
        Err(Errno::INTR) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn bytes_follow(reader: &PipeReader) -> Result<bool> {
    if !readable(reader, &NOW)? {
        return Ok(false);
    }
    let mut probe = [0_u8; 1];
    Ok(matches!((&*reader).read(&mut probe), Ok(count) if count > 0))
}

struct Limits {
    max_data: u64,
    budget: u64,
}

impl Limits {
    fn cap(&self) -> u64 {
        self.max_data.min(self.budget)
    }

    fn admit(&self, size: u64) -> Result<()> {
        if size <= self.cap() {
            return Ok(());
        }
        if self.budget < self.max_data {
            bail!(
                "the cache entry does not fit in the {} byte cache budget of this repository",
                self.budget
            );
        }
        Err(over_limit(size))
    }
}

fn pump(reader: &PipeReader, mut data: File, limits: &Limits, signal: &Signal) -> Result<Intake> {
    let limit = limits.cap();
    let mut buffer = vec![0_u8; CHUNK];
    let mut total = 0_u64;
    loop {
        if signal.aborted.load(Ordering::Relaxed) {
            bail!(Aborted);
        }
        if let Some(bytes) = signal.sealed.get().copied().filter(|bytes| total >= *bytes) {
            ensure!(
                !bytes_follow(reader)?,
                "the cache stream held more than the {bytes} bytes the sender sealed"
            );
            break;
        }
        if !readable(reader, &TICK)? {
            continue;
        }
        let room = signal
            .sealed
            .get()
            .map_or(u64::MAX, |bytes| bytes.saturating_sub(total));
        let want =
            usize::try_from(room.min(limit + 1 - total)).map_or(CHUNK, |room| room.min(CHUNK));
        match (&*reader).read(&mut buffer[..want]) {
            Ok(0) if want > 0 => break,
            Ok(count) => {
                data.write_all(&buffer[..count])?;
                total += u64::try_from(count)?;
                limits.admit(total)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Intake { data, bytes: total })
}

pub(super) fn begin(
    layout: &Layout,
    facts: &TrustedFacts,
    key: String,
    version: String,
) -> Result<(Session, PipeWriter), Response> {
    let scope = facts.cache_write_scope().map_err(Response::Denied)?;
    super::cache::check_request(&key, &version).map_err(failed)?;
    let (reader, writer) = std::io::pipe().map_err(failed)?;
    widen(&writer);
    let name = layout.next_name();
    let (upload, data) = layout.create_incoming(&name).map_err(failed)?;
    let signal = Arc::new(Signal::default());
    let intake = {
        let limits = Limits {
            max_data: layout.max_data,
            budget: layout.budget,
        };
        let signal = signal.clone();
        tokio::task::spawn_blocking(move || pump(&reader, data, &limits, &signal))
    };
    let session = Session {
        layout: layout.clone(),
        target: Target {
            owner: CacheOwner::of(facts),
            scope,
            key,
            version,
        },
        upload,
        signal,
        intake: Some(intake),
        armed: true,
    };
    Ok((session, writer))
}

impl Session {
    #[cfg(test)]
    pub(super) fn pump_finished(&self) -> bool {
        self.intake.as_ref().is_some_and(JoinHandle::is_finished)
    }

    async fn intake(&mut self) -> Result<Intake> {
        let Some(handle) = self.intake.take() else {
            bail!("the cache upload was already closed");
        };
        handle.await?
    }

    pub(super) async fn abort(mut self) -> Response {
        self.signal.aborted.store(true, Ordering::Relaxed);
        match self.intake().await {
            Err(error) if !error.is::<Aborted>() => failed(error),
            _ => Response::Done {
                notices: Vec::new(),
            },
        }
    }

    pub(super) async fn seal(mut self, bytes: u64) -> Response {
        let _ = self.signal.sealed.set(bytes);
        let intake = match self.intake().await {
            Ok(intake) if intake.bytes == bytes => intake,
            Ok(intake) => {
                return failed(format!(
                    "the cache stream held {} bytes, not the {bytes} the sender sealed",
                    intake.bytes
                ));
            }
            Err(error) => return failed(error),
        };
        let abandoned = Arc::new(AtomicBool::new(false));
        let _guard = Abandoned(abandoned.clone());
        self.armed = false;
        let (layout, target, upload) = (
            self.layout.clone(),
            self.target.clone(),
            self.upload.clone(),
        );
        tokio::task::spawn_blocking(move || {
            publish(&layout, &target, &upload, &intake.data, &abandoned)
        })
        .await
        .unwrap_or_else(|error| failed(format!("cache publish task failed: {error}")))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.armed {
            self.signal.aborted.store(true, Ordering::Relaxed);
            self.upload.discard();
        }
    }
}
