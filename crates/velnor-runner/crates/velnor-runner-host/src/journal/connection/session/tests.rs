//! Scoped session and host-wide header authority regressions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{CapacityLimits, ConnectionJournal, Journal, Outcome, SessionCreateState};
use crate::launch::{CompletedHeaderPoll, DeletedSession204};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-session-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn binding(id: &str, repository_id: i64) -> Result<RepositoryConnection, HostError> {
    let connection_id = ConnectionId::parse(id)?;
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    RepositoryConnection::try_new(
        connection_id,
        repository_id,
        format!("owner/{id}"),
        KeychainReference::parse(&format!("keychain:velnor/{id}"))?,
        selector,
        policy,
    )
}

async fn open_scope(
    path: &Path,
    id: &str,
    repository_id: i64,
    max_jobs: u32,
) -> Result<(Journal, ConnectionJournal), HostError> {
    let journal = Journal::open(path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: max_jobs,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await?;
    journal.bootstrap_header_authority().await?;
    journal.bind_engine("test-engine").await?;
    let binding = binding(id, repository_id)?;
    let scope = journal.connection_scope(&binding).await?;
    scope
        .bind_scale_set(repository_id + 10_000, "integration", SetOwnership::Adopted)
        .await?;
    Ok((journal, scope))
}

#[path = "tests/lifecycle_cases.rs"]
mod lifecycle_cases;
#[path = "tests/recovery_cases.rs"]
mod recovery_cases;
