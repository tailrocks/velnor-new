//! Temporary journal and queue-message fixtures for host tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics};

use crate::{HostError, Journal};

const CANARY: &str = "CANARYJIT";

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

pub(crate) fn assigned_wait(message_id: i64, assigned: i64) -> Poll {
    kind_wait(message_id, assigned, InnerKind::Assigned)
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
            source: velnor_runner_github::SourceMetadata::default(),
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
                source: velnor_runner_github::SourceMetadata::default(),
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
