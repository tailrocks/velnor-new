//! Scripted acquire, JIT, and ack transport for launch tests.
//!
//! Every item is `cfg(test)`: production builds see an empty module. The
//! canonical suite form bans `cfg(test)` module declarations, so shared
//! launch scaffolding lives here instead of a test-only module.

#[cfg(test)]
use std::future::Future;
use std::path::PathBuf;

#[cfg(test)]
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::Duration;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics};

#[cfg(test)]
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

#[cfg(test)]
use super::{Drive, Lane};
use velnor_runner_host::HostError;

#[cfg(test)]
use velnor_runner_host::{EnsureError, Journal};

#[cfg(test)]
pub(crate) const CANARY: &str = "CANARYJIT";

#[cfg(test)]
const TIMEOUT: Duration = Duration::from_secs(2);

/// Scratch tempdir shared with host-crate suites (always compiled).
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Create a unique tempdir for `label`. Removed on drop.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the tempdir cannot be created.
    pub fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-launch-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    /// Journal database path inside the scratch dir.
    #[must_use]
    pub fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.path);
        let _kept = cleanup.err().map(|err| err.kind());
    }
}

#[cfg(test)]
pub(crate) struct Script {
    pub(crate) calls: Vec<&'static str>,
    pub(crate) mode: Mode,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Ok,
    Empty,
    Timeout,
    Forbidden,
    AcquireMalformed,
    AcquireForeign,
    AcquireServerError,
    AckFail,
    JitFail,
    JitConflict,
    JitMalformed,
}

#[cfg(test)]
impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.path.contains("acquirejobs") {
            self.calls.push("acquire");
            return self.acquire(&request.body);
        }
        if request.path.contains("generatejitconfig") {
            self.calls.push("jit");
            if matches!(self.mode, Mode::JitConflict) {
                return Ok(Exchange {
                    status: 409,
                    body: Vec::new(),
                });
            }
            if matches!(self.mode, Mode::JitFail) {
                return Err(TransportFail::Http(500));
            }
            if matches!(self.mode, Mode::JitMalformed) {
                return Ok(Exchange {
                    status: 200,
                    body: br#"{"encodedJITConfig":""}"#.to_vec(),
                });
            }
            let body = format!(r#"{{"encodedJITConfig":"{CANARY}"}}"#);
            return Ok(Exchange {
                status: 200,
                body: body.into_bytes(),
            });
        }
        if request.method == Method::Delete {
            self.calls.push("ack");
            return self.ack();
        }
        Err(TransportFail::Http(500))
    }
}

#[cfg(test)]
impl Script {
    fn acquire(&self, body: &[u8]) -> Result<Exchange, TransportFail> {
        if matches!(self.mode, Mode::AcquireServerError) {
            return Err(TransportFail::Http(500));
        }
        if matches!(self.mode, Mode::Timeout) {
            return Err(TransportFail::Timeout);
        }
        if matches!(self.mode, Mode::Forbidden) {
            return Err(TransportFail::Http(403));
        }
        if matches!(self.mode, Mode::AcquireMalformed) {
            return Ok(Exchange {
                status: 200,
                body: b"not-json".to_vec(),
            });
        }
        let ids: Vec<i64> = serde_json::from_slice(body).map_err(|_| TransportFail::Http(400))?;
        let value = if matches!(self.mode, Mode::Empty) {
            Vec::new()
        } else if matches!(self.mode, Mode::AcquireForeign) {
            vec![i64::MAX]
        } else {
            ids
        };
        let count = i64::try_from(value.len()).map_err(|_| TransportFail::Http(400))?;
        let text = serde_json::json!({ "count": count, "value": value }).to_string();
        Ok(Exchange {
            status: 200,
            body: text.into_bytes(),
        })
    }

    fn ack(&self) -> Result<Exchange, TransportFail> {
        if matches!(self.mode, Mode::AckFail) {
            return Err(TransportFail::Http(500));
        }
        Ok(Exchange {
            status: 204,
            body: Vec::new(),
        })
    }
}

#[cfg(test)]
impl Lane for Script {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn ctx() -> Drive {
    Drive {
        set_id: 1,
        queue_path: "queues/messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
    }
}

#[cfg(test)]
pub(crate) fn started_progress(message_id: i64, assigned: i64) -> Poll {
    let mut poll = assigned_wait(message_id, assigned);
    let Poll::Batch(batch) = &mut poll else {
        return poll;
    };
    batch.jobs = vec![
        progress_job(InnerKind::Started),
        progress_job(InnerKind::Started),
    ];
    poll
}

#[cfg(test)]
fn progress_job(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: Some(0),
        job_id: None,
        workflow_run_id: None,
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: None,
        runner_name: None,
        result: None,
        fields: Vec::new(),
    }
}

/// Assigned-poll fixture shared with host-crate suites (always compiled).
#[must_use]
pub fn assigned_wait(message_id: i64, assigned: i64) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        statistics: Some(Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs: assigned,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        }),
        jobs: vec![InnerJob {
            kind: InnerKind::Assigned,
            request_id: Some(0),
            job_id: None,
            workflow_run_id: None,
            owner_name: None,
            repository_name: None,
            event_name: None,
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
        }],
    })
}

#[cfg(test)]
pub(crate) fn available(ids: &[i64]) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 4,
        statistics: None,
        jobs: ids
            .iter()
            .copied()
            .map(|id| InnerJob {
                kind: InnerKind::Available,
                request_id: Some(id),
                job_id: None,
                workflow_run_id: None,
                owner_name: None,
                repository_name: None,
                event_name: None,
                labels: Vec::new(),
                runner_id: None,
                runner_name: None,
                result: None,
                fields: Vec::new(),
            })
            .collect(),
    })
}

#[cfg(test)]
pub(crate) async fn open(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|err| err.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|err| err.to_string())?;
    Ok((scratch, journal))
}

#[cfg(test)]
pub(crate) fn absent(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains(CANARY) || text.contains("queue-token") || text.contains("admin-token") {
        return Err("journal stored a secret".to_owned());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|err| err.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|err| err.to_string())?;
    Ok((scratch, journal))
}

#[cfg(test)]
pub(crate) async fn launch_row(journal: &Journal) -> Result<i64, String> {
    launch_row_for_id(journal, "job", "runner-id").await
}

#[cfg(test)]
pub(crate) async fn launch_row_for_id(
    journal: &Journal,
    subject: &str,
    docker_id: &str,
) -> Result<i64, String> {
    let row_id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind(row_id, Some(docker_id), None)
        .await
        .map_err(|err| err.to_string())?;
    Ok(row_id)
}

#[cfg(test)]
pub(crate) async fn within<F: Future>(future: F, label: &str) -> Result<F::Output, String> {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .map_err(|_| format!("{label} timed out"))
}

#[cfg(test)]
pub(crate) fn no_response_body_in_journal(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains("private runner-id detail") {
        return Err("journal stored Docker response data".to_owned());
    }
    Ok(())
}
