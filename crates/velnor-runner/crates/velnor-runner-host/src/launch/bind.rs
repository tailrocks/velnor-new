//! Journal binding for one pair. Ids are committed before the container starts.

use super::{SelectedEngineContext, VerifiedWorkerImages};
use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::{ConnectionJournal, Journal};
use crate::stage::{PairSink, PairStop, StageAdmission, drive};
use crate::worker::guest_image::GuestImageFile;
use crate::worker::{ResourceBudget, Started};

/// Owned journal handle for the row this pair belongs to.
#[derive(Debug, Clone)]
pub(crate) struct Bind {
    journal: PairJournal,
    row: i64,
}

#[derive(Debug, Clone)]
enum PairJournal {
    Host(Journal),
    Connection(ConnectionJournal),
}

impl Bind {
    pub(crate) fn new(journal: &Journal, row: i64) -> Self {
        Self {
            journal: PairJournal::Host(journal.clone()),
            row,
        }
    }

    pub(crate) fn scoped(journal: &ConnectionJournal, row: i64) -> Self {
        Self {
            journal: PairJournal::Connection(journal.clone()),
            row,
        }
    }
}

impl PairSink for Bind {
    async fn volume(&self, volume: &str) -> Result<(), HostError> {
        match &self.journal {
            PairJournal::Host(journal) => journal.bind_worker_volume(self.row, volume).await,
            PairJournal::Connection(journal) => journal.bind_worker_volume(self.row, volume).await,
        }
    }

    async fn dind(&self, id: &str) -> Result<(), HostError> {
        match &self.journal {
            PairJournal::Host(journal) => journal.bind_worker(self.row, None, Some(id)).await,
            PairJournal::Connection(journal) => journal.bind_worker(self.row, None, Some(id)).await,
        }
    }

    async fn runner(&self, id: &str) -> Result<(), HostError> {
        match &self.journal {
            PairJournal::Host(journal) => journal.bind_worker(self.row, Some(id), None).await,
            PairJournal::Connection(journal) => journal.bind_worker(self.row, Some(id), None).await,
        }
    }
}

pub(super) async fn start_bound(
    docker: &bollard::Docker,
    volume: &str,
    jit: &[u8],
    resource_budget: ResourceBudget,
    connection: &RepositoryConnection,
    images: &VerifiedWorkerImages,
    expected_engine: &SelectedEngineContext,
    guest_image: &GuestImageFile,
    admission: StageAdmission,
    bind: &Bind,
) -> Result<Started, HostError> {
    let partial = Box::pin(drive(
        docker,
        volume,
        resource_budget,
        jit,
        PairStop::Jit,
        connection,
        images,
        expected_engine,
        guest_image,
        admission,
        bind,
    ))
    .await?;
    Ok(Started {
        dind_id: partial.dind_id.ok_or(HostError::Docker)?,
        runner_id: partial.runner_id.ok_or(HostError::Docker)?,
    })
}
