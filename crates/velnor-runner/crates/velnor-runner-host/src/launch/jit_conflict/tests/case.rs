// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, Pair, TestResult, binding, github::Github, probe_docker::FixtureDocker,
    scalar, seed, seed_jit, storage_seed,
};
use crate::launch::bootstrap::NativeCleanupRuntime;
use crate::{
    config::RepositoryConnection,
    journal::{ConnectionJournal, JitConflictRecoveryClaim, Journal, MetadataSettlementResult},
    launch::{ImageAuthority, SelectedEngineContext, VerifiedWorkerImages},
    listen::Secret,
};
use bollard::Docker;
use std::sync::Arc;

const IMAGE: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const COLUMNS: &str = "lineage_id,generation,reservation_id,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,authority_generation,authority_digest,engine_id,docker_root_digest,requested_name,created_at";
pub(super) const FOREIGN: Pair = Pair {
    reservation: 100,
    bound: false,
};

#[derive(Clone, Copy)]
pub(super) enum Schema {
    Current,
    Historical45,
    Historical46,
    Historical47,
}

pub(super) struct Case {
    directory: tempfile::TempDir,
    pub(super) fixture: FixtureDocker,
    pub(super) docker: Docker,
    pub(super) journal: Journal,
    pub(super) scope: ConnectionJournal,
    pub(super) engine: SelectedEngineContext,
    pub(super) binding: RepositoryConnection,
    pub(super) images: VerifiedWorkerImages,
    pub(super) authority: Arc<ImageAuthority>,
}

impl Case {
    pub(super) async fn new(
        schema: Schema,
        outcome: JitOutcome,
        pending: bool,
    ) -> TestResult<Self> {
        Self::with_control(schema, outcome, pending, false).await
    }

    pub(super) async fn with_control(
        schema: Schema,
        outcome: JitOutcome,
        pending: bool,
        control: bool,
    ) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("docker.sock");
        let mut pairs = vec![CRASH, FOREIGN];
        if control {
            pairs.push(super::policy_rotation::CONTROL);
        }
        let fixture = FixtureDocker::start(&socket, &pairs)?;
        let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
        let path = directory.path().join("launch.db");
        let journal = match schema {
            Schema::Current => Journal::open(&path).await?,
            Schema::Historical45 => Journal::open_historical_v45_for_test(&path).await?,
            Schema::Historical46 => Journal::open_historical_v46_for_test(&path).await?,
            Schema::Historical47 => Journal::open_historical_v47_for_test(&path).await?,
        };
        let started = if control {
            vec![CRASH, super::policy_rotation::CONTROL]
        } else {
            vec![CRASH]
        };
        let (scope, engine) = seed::started_pairs(&journal, &docker, &started).await?;
        seed::bind_resources(&journal, CRASH, engine.docker_root_digest()).await?;
        binding::install(&journal, CRASH).await?;
        storage_seed::install(&journal, CRASH).await?;
        seed_jit(&journal, CRASH, outcome).await?;
        Self::seed_control(&journal, &engine, control).await?;
        if pending {
            super::population::seed_pending(&journal).await?;
        }
        let binding: RepositoryConnection = serde_json::from_str(
            &scalar(&journal, "SELECT identity FROM connection_scopes LIMIT 1").await?,
        )?;
        if matches!(schema, Schema::Historical45) {
            seed::execute(&journal,&format!("CREATE VIEW physical_attempt_jit_recovery_sources AS SELECT {COLUMNS},'conflict' AS source_kind FROM physical_attempt_jit_conflict_receipts WHERE response_status=409")).await?;
        }
        let images = VerifiedWorkerImages::for_test(&engine, &"a".repeat(40), IMAGE, IMAGE)?;
        let authority = Arc::new(ImageAuthority::Developer(serde_json::from_value(
            serde_json::json!({"runner":IMAGE,"dind":IMAGE,"resource_probe":IMAGE}),
        )?));
        Ok(Self {
            directory,
            fixture,
            docker,
            journal,
            scope,
            engine,
            binding,
            images,
            authority,
        })
    }

    async fn seed_control(
        journal: &Journal,
        engine: &SelectedEngineContext,
        control: bool,
    ) -> TestResult<()> {
        if control {
            let pair = super::policy_rotation::CONTROL;
            seed::bind_resources(journal, pair, engine.docker_root_digest()).await?;
            binding::install(journal, pair).await?;
            storage_seed::install(journal, pair).await?;
            super::seed_authorized_bound_request(journal, pair).await?;
        }
        Ok(())
    }

    pub(super) fn runtime(&self) -> TestResult<NativeCleanupRuntime> {
        Ok(NativeCleanupRuntime::new(
            self.docker.clone(),
            self.journal.clone(),
            crate::journal::EngineContext {
                engine_id: self.engine.engine_id().to_owned(),
                endpoint: self.engine.endpoint().to_owned(),
                platform: self.engine.platform().to_owned(),
            },
            self.engine.clone(),
            Arc::clone(&self.authority),
        )?)
    }

    pub(super) async fn reopen(self, schema: Schema) -> TestResult<Self> {
        let Self {
            directory,
            fixture,
            docker,
            journal,
            scope,
            engine,
            binding,
            images,
            authority,
        } = self;
        drop(scope);
        drop(journal);
        let path = directory.path().join("launch.db");
        let journal = match schema {
            Schema::Current => Journal::open(&path).await?,
            Schema::Historical45 => Journal::open_historical_v45_for_test(&path).await?,
            Schema::Historical46 => Journal::open_historical_v46_for_test(&path).await?,
            Schema::Historical47 => Journal::open_historical_v47_for_test(&path).await?,
        };
        let scope = journal.connection_scope(&binding).await?;
        Ok(Self {
            directory,
            fixture,
            docker,
            journal,
            scope,
            engine,
            binding,
            images,
            authority,
        })
    }

    pub(super) async fn remote_and_network(
        &self,
        claim: &JitConflictRecoveryClaim,
        exists: bool,
    ) -> TestResult<()> {
        let name = format!("v{}", CRASH.nonce());
        let mut admin = Github::new(&name, exists);
        let mut identity = Github::new(&name, false);
        let mut link = admin.link(&self.binding)?;
        let pat = Secret::new("pat");
        let context = super::super::ReconcileContext {
            docker: &self.docker,
            journal: &self.journal,
            scope: &self.scope,
            binding: &self.binding,
            set_id: 7,
            link: &mut link,
            github_pat: &pat,
            selected: &self.engine,
            images: &self.images,
            image_authority: &self.authority,
        };
        assert!(
            super::super::remote::reconcile_admin(
                &context,
                claim,
                &mut identity,
                &mut admin,
                &Secret::new("admin")
            )
            .await?
        );
        assert!(admin.runners.contains_key(&99));
        assert!(!admin.runners.contains_key(&21));
        assert!(admin.lookups.iter().all(|value| value == &name));
        super::super::ensure_network_absence(&self.docker, &self.scope, &self.engine, claim)
            .await?;
        Ok(())
    }

    pub(super) async fn settle(&self) -> TestResult<()> {
        assert_eq!(
            self.runtime()?
                .settle_storage_metadata_after_cleanup(&self.scope, CRASH.reservation)
                .await?,
            MetadataSettlementResult::Settled
        );
        assert_eq!(self.fixture.objects(CRASH), Vec::<String>::new());
        Ok(())
    }

    pub(super) async fn finish(
        &self,
        claim: &JitConflictRecoveryClaim,
        exists: bool,
    ) -> TestResult<()> {
        self.remote_and_network(claim, exists).await?;
        self.settle().await
    }
}
