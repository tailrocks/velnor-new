//! A launch row without worker IDs stays occupied until cleanup is proven.

use crate::launch::{Admit, admission};
use crate::launch_harness::{absent, assigned_wait, open};
use crate::stage::PairEngine;
use crate::worker::CreateProjection;
use crate::{HostError, Journal, Outcome};

struct Idle;

#[expect(
    clippy::unused_async_trait_impl,
    reason = "the trait is async and this engine never awaits"
)]
impl PairEngine for Idle {
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

    async fn running(&self, _id: &str) -> Result<bool, HostError> {
        Ok(false)
    }
}

async fn uncertain_without_ids(journal: &Journal, subject: &str) -> Result<(), String> {
    let id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())
}

#[tokio::test]
async fn idless_uncertain_rows_keep_both_slots() -> Result<(), String> {
    let (scratch, journal) = open("idless").await?;
    uncertain_without_ids(&journal, "m1").await?;
    uncertain_without_ids(&journal, "m2").await?;
    let decision = admission(&Idle, &journal, 2, 2, 0, &assigned_wait(9, 2))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| !row.cleanup_proven));
    absent(&scratch.file())
}
