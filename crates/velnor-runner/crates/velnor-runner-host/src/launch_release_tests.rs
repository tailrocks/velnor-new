//! `release_slots` proves a gone runner and keeps a running one.

use std::collections::BTreeSet;

use crate::launch::release_slots;
use crate::launch_harness::{absent, open};
use crate::stage::PairEngine;
use crate::worker::CreateProjection;
use crate::{HostError, IntentRow, IntentState, Journal, Outcome};

struct Seen {
    running: BTreeSet<String>,
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait and does not await"
)]
impl PairEngine for Seen {
    async fn prepare_volumes(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        Ok(String::new())
    }

    async fn start(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Ok(())
    }

    async fn remove(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn id_for_name(&self, _name: &str) -> Result<Option<String>, HostError> {
        Ok(None)
    }

    async fn worker_id_for_name(
        &self,
        _name: &str,
        _volume: &str,
        _role: &str,
    ) -> Result<Option<String>, HostError> {
        Ok(None)
    }

    async fn remove_worker_volumes(&self, _volume: &str) -> Result<bool, HostError> {
        Ok(true)
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        Ok(self.running.contains(id))
    }
}

fn hex(n: u64) -> String {
    format!("{n:064x}")
}

async fn occupy(journal: &Journal, subject: &str, docker_id: &str) -> Result<(), String> {
    let id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind(id, Some(docker_id), None)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|err| err.to_string())
}

fn row<'a>(rows: &'a [IntentRow], docker_id: &str) -> Result<&'a IntentRow, String> {
    rows.iter()
        .find(|row| row.docker_id.as_deref() == Some(docker_id))
        .ok_or_else(|| "row".to_owned())
}

#[tokio::test]
async fn release_slots_proves_gone_and_keeps_running() -> Result<(), String> {
    let (scratch, journal) = open("release-slots").await?;
    let gone = hex(1);
    let live = hex(2);
    assert_eq!(gone.len(), 64);
    assert_eq!(live.len(), 64);
    occupy(&journal, "gone", &gone).await?;
    occupy(&journal, "live", &live).await?;
    let engine = Seen {
        running: BTreeSet::from([live.clone()]),
    };
    release_slots(&journal, &engine)
        .await
        .map_err(|err| err.to_string())?;
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let gone_row = row(&rows, &gone)?;
    let live_row = row(&rows, &live)?;
    assert_eq!(gone_row.state, IntentState::Done);
    assert!(gone_row.cleanup_proven);
    assert_eq!(gone_row.dind_id, None);
    assert_eq!(live_row.state, IntentState::Done);
    assert!(!live_row.cleanup_proven);
    assert_eq!(live_row.dind_id, None);
    absent(&scratch.file())
}
