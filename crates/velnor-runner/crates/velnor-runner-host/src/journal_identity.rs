//! Immutable engine, launch, and resource identities in the journal.

use crate::daemon_lock::{EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity};
use crate::journal_schema;
use crate::journal_sql::{one_row, token_rejected};

impl Journal {
    /// Bind this journal to one validated, engine-scoped external lineage.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a changed engine or restored state.
    pub(crate) async fn establish_engine_lineage(
        &self,
        engine_id: &str,
        guard: EngineLineageGuard,
    ) -> Result<(), HostError> {
        if self
            .engine_binding()
            .await?
            .is_some_and(|stored| stored != engine_id)
        {
            return Err(HostError::Journal);
        }
        let path = canonical_journal_path(self.path())?;
        let instance_id = self.instance_id().await?;
        let revision = self.revision().await?;
        if self.lineage_pinned().await? {
            guard.verify_existing_lineage(&path, &instance_id, revision)?;
        } else {
            guard.verify_lineage(&path, &instance_id, revision)?;
        }
        self.attach_lineage_guard(guard)?;
        self.bind_engine(engine_id).await?;
        self.pin_lineage().await
    }

    /// Bind this journal to one Docker engine. A changed engine fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the id is invalid or changes.
    pub async fn bind_engine(&self, engine_id: &str) -> Result<(), HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let result = journal_schema::bind_engine(&conn, engine_id).await;
        self.sync_after(result).await
    }

    /// Read the journal's stable instance identity.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the identity is absent.
    pub(crate) async fn instance_id(&self) -> Result<String, HostError> {
        let conn = self.connection().await?;
        journal_schema::instance_id(&conn).await
    }

    /// Read the monotonic state revision committed with journal writes.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the revision is absent or invalid.
    pub(crate) async fn revision(&self) -> Result<u64, HostError> {
        let conn = self.connection().await?;
        journal_schema::revision(&conn).await
    }

    async fn engine_binding(&self) -> Result<Option<String>, HostError> {
        let conn = self.connection().await?;
        journal_schema::engine_id_optional(&conn).await
    }

    async fn lineage_pinned(&self) -> Result<bool, HostError> {
        let conn = self.connection().await?;
        journal_schema::lineage_pinned(&conn).await
    }

    async fn pin_lineage(&self) -> Result<(), HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE journal_meta SET lineage_pinned = 1 WHERE singleton = 1 AND lineage_pinned = 0",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            self.sync_lineage().await
        } else if self.lineage_pinned().await? {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    }

    /// Read one launch identity after the engine is bound.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a legacy row or an unbound engine.
    pub async fn launch_identity(&self, id: i64) -> Result<LaunchIdentity, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT launch_id FROM intents WHERE id = ?1 AND kind = 'launch'",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let launch_id: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
        let launch_id = launch_id.ok_or(HostError::Journal)?;
        let instance_id = journal_schema::instance_id(&conn).await?;
        let engine_id = journal_schema::engine_id(&conn).await?;
        LaunchIdentity::new(&instance_id, id, &launch_id, &engine_id)
    }

    /// Bind both immutable Docker ids. Rebinding to another id fails.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when either id conflicts or the row is missing.
    pub async fn bind_pair(
        &self,
        id: i64,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(runner_id) || token_rejected(dind_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (docker_id IS NULL OR docker_id = ?1) AND (dind_id IS NULL OR dind_id = ?2)",
                (runner_id, dind_id, id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }

    /// Bind the runner container id once. A different id is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row or identity conflicts.
    pub(crate) async fn bind_runner_container(
        &self,
        id: i64,
        docker_id: &str,
    ) -> Result<(), HostError> {
        self.bind_launch_container_column(id, "docker_id", docker_id)
            .await
    }

    /// Bind the DinD container id once. A different id is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row or identity conflicts.
    pub(crate) async fn bind_dind_container(
        &self,
        id: i64,
        docker_id: &str,
    ) -> Result<(), HostError> {
        self.bind_launch_container_column(id, "dind_id", docker_id)
            .await
    }

    async fn bind_launch_container_column(
        &self,
        id: i64,
        column: &str,
        docker_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(docker_id) || !matches!(column, "docker_id" | "dind_id") {
            return Err(HostError::Journal);
        }
        let sql = format!(
            "UPDATE intents SET {column} = COALESCE({column}, ?1) WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND ({column} IS NULL OR {column} = ?1)"
        );
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(&sql, (docker_id, id))
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }

    /// Bind one immutable GitHub runner id to a launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row or identity conflicts.
    pub(crate) async fn bind_github_runner(
        &self,
        id: i64,
        github_runner_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(github_runner_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET github_runner_id = COALESCE(github_runner_id, ?1) WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND jit_requested = 1 AND (github_runner_id IS NULL OR github_runner_id = ?1)",
                (github_runner_id, id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }

    /// Bind one immutable seed generation to a launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when a different generation is already bound.
    pub async fn bind_seed_generation(
        &self,
        id: i64,
        generation_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(generation_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET seed_generation_id = COALESCE(seed_generation_id, ?1) WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND (seed_generation_id IS NULL OR seed_generation_id = ?1)",
                (generation_id, id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }
}
