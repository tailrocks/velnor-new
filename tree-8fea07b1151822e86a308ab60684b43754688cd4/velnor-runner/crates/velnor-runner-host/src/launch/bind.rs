//! Journal binding for one pair. Ids are committed before the container starts.

use crate::error::HostError;
use crate::journal::Journal;
use crate::stage::{PairSink, PairStop, drive};
use crate::worker::Started;

/// Owned journal handle for the row this pair belongs to.
#[derive(Debug, Clone)]
pub(crate) struct Bind {
    journal: Journal,
    row: i64,
}

impl Bind {
    pub(crate) fn new(journal: &Journal, row: i64) -> Self {
        Self {
            journal: journal.clone(),
            row,
        }
    }
}

impl PairSink for Bind {
    async fn volume(&self, volume: &str) -> Result<(), HostError> {
        self.journal.bind_worker_volume(self.row, volume).await
    }

    async fn dind(&self, id: &str) -> Result<(), HostError> {
        self.journal.bind_worker(self.row, None, Some(id)).await
    }

    async fn runner(&self, id: &str) -> Result<(), HostError> {
        self.journal.bind_worker(self.row, Some(id), None).await
    }
}

pub(super) async fn start_bound(
    docker: &bollard::Docker,
    volume: &str,
    jit: &[u8],
    bind: &Bind,
) -> Result<Started, HostError> {
    let partial = Box::pin(drive(docker, volume, jit, PairStop::Jit, bind)).await?;
    Ok(Started {
        dind_id: partial.dind_id.ok_or(HostError::Docker)?,
        runner_id: partial.runner_id.ok_or(HostError::Docker)?,
    })
}
