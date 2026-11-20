// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use bollard::models::SystemInfo;
use velnor_runner_github::{QueueSession, SessionError};

use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{EngineContext, Journal, SessionRefreshClaim};
use crate::launch::SelectedEngineContext;
use crate::scale_set::EnsureError;

#[derive(Debug)]
pub(crate) struct VerifiedSessionRefresh {
    session_intent_id: i64,
    session_id: String,
    generation: i64,
}

impl VerifiedSessionRefresh {
    pub(super) fn after_response(
        claim: &SessionRefreshClaim,
        session: &QueueSession,
    ) -> Result<Self, EnsureError> {
        if session.session_id != claim.session_id() {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self {
            session_intent_id: claim.session_intent_id(),
            session_id: claim.session_id().to_owned(),
            generation: claim.generation(),
        })
    }

    pub(crate) fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }
}

#[derive(Debug)]
pub(crate) struct ReturnedSessionRefreshTransportError {
    session_intent_id: i64,
    session_id: String,
    generation: i64,
}

impl ReturnedSessionRefreshTransportError {
    pub(super) fn from_returned_result(
        claim: &SessionRefreshClaim,
        result: &Result<QueueSession, SessionError>,
    ) -> Option<Self> {
        matches!(result, Err(SessionError::Uncertain)).then(|| Self {
            session_intent_id: claim.session_intent_id(),
            session_id: claim.session_id().to_owned(),
            generation: claim.generation(),
        })
    }

    pub(crate) fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }
}

#[derive(Debug)]
pub(crate) struct SessionRefreshStartupAuthority<'a> {
    daemon_lock: &'a DaemonLock,
    daemon_lock_path: PathBuf,
    journal_path: PathBuf,
    instance_id: String,
    engine_id: String,
    revision: u64,
    lineage_guard: &'a EngineLineageGuard,
}

impl SessionRefreshStartupAuthority<'_> {
    pub(crate) async fn validate_lock_and_lineage(
        &self,
        journal: &Journal,
    ) -> Result<(), HostError> {
        if !self.daemon_lock.matches_path(&self.daemon_lock_path) {
            return Err(HostError::Lock);
        }
        journal
            .validate_startup_attachment(
                &self.journal_path,
                &self.instance_id,
                &self.engine_id,
                self.revision,
                self.lineage_guard,
            )
            .await
    }

    pub(crate) fn canonical_journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

impl<'a> SessionRefreshStartupAuthority<'a> {
    pub(crate) async fn after_validated_startup(
        daemon_lock: &'a DaemonLock,
        state: &Path,
        lineage_guard: &'a EngineLineageGuard,
        journal: &Journal,
        selected_engine: &SelectedEngineContext,
        fresh_info: &SystemInfo,
    ) -> Result<Self, HostError> {
        let canonical_state = fs::canonicalize(state).map_err(|_| HostError::Path)?;
        if !fs::metadata(&canonical_state)
            .map_err(|_| HostError::Path)?
            .is_dir()
        {
            return Err(HostError::Path);
        }
        let daemon_lock_path = canonical_state.join("daemon.lock");
        if !daemon_lock.matches_path(&daemon_lock_path) {
            return Err(HostError::Lock);
        }

        let selection = EngineContext {
            engine_id: selected_engine.engine_id().to_owned(),
            endpoint: selected_engine.endpoint().to_owned(),
            platform: selected_engine.platform().to_owned(),
        };
        let current = SelectedEngineContext::capture(&selection, fresh_info)?;
        if current != *selected_engine {
            return Err(HostError::Ownership);
        }

        let journal_path = canonical_journal_path(&canonical_state.join("launch.db"))?;
        let instance_id = journal.instance_id().await?;
        let engine_id = journal.engine_id().await?;
        let revision = journal.revision().await?;
        if engine_id != selected_engine.engine_id() || lineage_guard.engine_id() != engine_id {
            return Err(HostError::Ownership);
        }
        lineage_guard.verify_lineage(&journal_path, &instance_id, revision)?;

        let authority = Self {
            daemon_lock,
            daemon_lock_path,
            journal_path,
            instance_id,
            engine_id,
            revision,
            lineage_guard,
        };
        authority.validate_lock_and_lineage(journal).await?;
        Ok(authority)
    }
}
