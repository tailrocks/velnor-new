//! Scripted acquire, JIT, and ack transport for launch tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_github::{
    Exchange, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest, Statistics,
    Transport, TransportFail,
};

use crate::launch::{Drive, Lane};
use crate::{EnsureError, HostError, Journal};

pub(crate) const CANARY: &str = "CANARYJIT";

pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub(crate) fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-launch-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    pub(crate) fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.path);
        let _kept = cleanup.err().map(|err| err.kind());
    }
}

pub(crate) struct Script {
    pub(crate) calls: Vec<&'static str>,
    pub(crate) mode: Mode,
}

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
    JitForbidden,
    JitConflict,
    JitMalformed,
    /// Offline idle runner `m100000788` id 20231. Delete returns 204.
    OfflineRunner,
    /// Online busy runner. Delete must not be called.
    BusyRunner,
    /// Runner list returns HTTP 500.
    ListFail,
    /// Delete returns HTTP 500 after an offline list.
    DeleteFail,
    /// First JIT is HTTP 409. The directory then deletes the offline runner.
    NameTakenOnce,
    /// Every JIT is HTTP 409. The directory deletes the offline runner.
    NameTaken,
    /// Every JIT is HTTP 409 for job runner `v7` id 20246.
    JobNameTaken,
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.path.contains("acquirejobs") {
            self.calls.push("acquire");
            return self.acquire(&request.body);
        }
        if request.path.contains("generatejitconfig") {
            let prior = self.calls.iter().filter(|&&item| item == "jit").count();
            self.calls.push("jit");
            if matches!(self.mode, Mode::JitConflict) {
                return Ok(Exchange {
                    status: 409,
                    body: Vec::new(),
                });
            }
            if matches!(self.mode, Mode::JitForbidden) {
                return Ok(Exchange {
                    status: 403,
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
            if matches!(self.mode, Mode::NameTaken | Mode::JobNameTaken)
                || (matches!(self.mode, Mode::NameTakenOnce) && prior == 0)
            {
                return Ok(Exchange {
                    status: 409,
                    body: Vec::new(),
                });
            }
            let body = format!(
                r#"{{"encodedJITConfig":"{CANARY}","runner":{{"id":31,"name":"runner-31","runnerScaleSetId":7}}}}"#
            );
            return Ok(Exchange {
                status: 200,
                body: body.into_bytes(),
            });
        }
        if request.path.contains("/actions/runners") && !request.path.contains("registration-token")
        {
            return Ok(self.runners(request.method));
        }
        if request.method == Method::Delete {
            self.calls.push("ack");
            return self.ack();
        }
        Err(TransportFail::Http(500))
    }
}

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

    fn runners(&mut self, method: Method) -> Exchange {
        let delete = method == Method::Delete;
        self.calls.push(if delete {
            "runner-delete"
        } else {
            "runners-list"
        });
        if matches!(self.mode, Mode::ListFail) && !delete {
            return Exchange {
                status: 500,
                body: Vec::new(),
            };
        }
        if delete {
            let status = if matches!(self.mode, Mode::DeleteFail) {
                500
            } else {
                204
            };
            return Exchange {
                status,
                body: Vec::new(),
            };
        }
        let busy = matches!(self.mode, Mode::BusyRunner);
        let status = if busy { "online" } else { "offline" };
        let (id, name) = if matches!(self.mode, Mode::JobNameTaken) {
            (20246, "v7")
        } else {
            (20231, "m100000788")
        };
        let body = format!(
            r#"{{"total_count":1,"runners":[{{"id":{id},"name":"{name}","status":"{status}","busy":{busy}}}]}}"#
        );
        Exchange {
            status: 200,
            body: body.into_bytes(),
        }
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

impl Lane for Script {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

pub(crate) fn ctx() -> Drive {
    Drive {
        set_id: 1,
        queue_path: "queues/messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
        docker_engine_id: None,
        owner: String::new(),
        repo: String::new(),
        pat: String::new(),
    }
}

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

fn progress_job(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: Some(0),
        job_id: None,
        labels: Vec::new(),
        runner_id: None,
        runner_name: None,
        result: None,
        fields: Vec::new(),
    }
}

pub(crate) fn assigned_wait(message_id: i64, assigned: i64) -> Poll {
    kind_wait(message_id, assigned, InnerKind::Assigned)
}

pub(crate) fn started_wait(message_id: i64, assigned: i64) -> Poll {
    kind_wait(message_id, assigned, InnerKind::Started)
}

fn kind_wait(message_id: i64, assigned: i64, kind: InnerKind) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: String::new(),
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
            kind,
            request_id: Some(0),
            job_id: None,
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
        }],
    })
}

pub(crate) fn available(ids: &[i64]) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 4,
        raw_body: String::new(),
        statistics: None,
        jobs: ids
            .iter()
            .copied()
            .map(|id| InnerJob {
                kind: InnerKind::Available,
                request_id: Some(id),
                job_id: None,
                labels: Vec::new(),
                runner_id: None,
                runner_name: None,
                result: None,
                fields: Vec::new(),
            })
            .collect(),
    })
}

pub(crate) async fn open(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|err| err.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|err| err.to_string())?;
    Ok((scratch, journal))
}

pub(crate) struct JitProbe {
    inner: Script,
    pub(crate) names: Vec<String>,
    conflict: bool,
}

impl JitProbe {
    pub(crate) fn conflict() -> Self {
        Self {
            inner: Script {
                calls: Vec::new(),
                mode: Mode::Ok,
            },
            names: Vec::new(),
            conflict: true,
        }
    }

    pub(crate) fn calls(&self) -> &[&'static str] {
        &self.inner.calls
    }
}

impl Transport for JitProbe {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.path.contains("generatejitconfig") {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&request.body)
                && let Some(name) = value.get("name").and_then(|item| item.as_str())
            {
                self.names.push(name.to_owned());
            }
            if self.conflict {
                self.inner.calls.push("jit");
                return Err(TransportFail::Http(409));
            }
        }
        self.inner.exchange(request)
    }
}

impl Lane for JitProbe {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        self.inner.on_admin()
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        self.inner.on_queue()
    }

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        self.inner.use_github_api()
    }
}

pub(crate) fn absent(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains(CANARY) || text.contains("queue-token") || text.contains("admin-token") {
        return Err("journal stored a secret".to_owned());
    }
    Ok(())
}
