//! Scripted acquire, JIT, and ack transport for launch tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_github::{
    Exchange, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest, Transport,
    TransportFail,
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

pub(crate) enum Mode {
    Ok,
    Empty,
    Timeout,
    Forbidden,
    AckFail,
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.path.contains("acquirejobs") {
            self.calls.push("acquire");
            return self.acquire(&request.body);
        }
        if request.path.contains("generatejitconfig") {
            self.calls.push("jit");
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

impl Script {
    fn acquire(&self, body: &[u8]) -> Result<Exchange, TransportFail> {
        if matches!(self.mode, Mode::Timeout) {
            return Err(TransportFail::Timeout);
        }
        if matches!(self.mode, Mode::Forbidden) {
            return Err(TransportFail::Http(403));
        }
        let ids: Vec<i64> = serde_json::from_slice(body).map_err(|_| TransportFail::Http(400))?;
        let value = if matches!(self.mode, Mode::Empty) {
            Vec::new()
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

impl Lane for Script {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

pub(crate) fn ctx() -> Drive {
    Drive {
        set_id: 1,
        queue_path: "queues/messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
    }
}

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

pub(crate) fn absent(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains(CANARY) || text.contains("queue-token") || text.contains("admin-token") {
        return Err("journal stored a secret".to_owned());
    }
    Ok(())
}
