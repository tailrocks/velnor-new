// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use bollard::Docker;
use tempfile::TempDir;

use crate::config::{ConnectionId, HostConfig};
use crate::connect::SetOwnership;
use crate::host_controller::loop_driver::state::{
    AdminRefreshSchedule, PreparedConnection, PreparedSetup,
};
use crate::journal::{ConnectionJournal, Journal};
use crate::launch::scoped_runtime::RunnerReadyRegistry;
use crate::launch::{ImageAuthority, SelectedEngineContext, VerifiedWorkerImages};
use crate::scale_set::EnsuredSet;
use crate::worker::guest_image::write_guest_image;

use super::seed::{CONNECTION_ID, IMAGE_ID};
use super::{HOST_CONFIG, PositivePollContext, TestResult};

pub(super) struct Controller {
    pub(super) config: HostConfig,
    pub(super) images: VerifiedWorkerImages,
    pub(super) image_authority: Arc<ImageAuthority>,
    pub(super) state: TempDir,
    pub(super) prepared: PreparedConnection,
}

impl Controller {
    pub(super) fn new(
        scope: &ConnectionJournal,
        engine: &SelectedEngineContext,
    ) -> TestResult<Self> {
        let config = HostConfig::parse(HOST_CONFIG)?;
        let images = VerifiedWorkerImages::for_test(engine, &"1".repeat(40), IMAGE_ID, IMAGE_ID)?;
        let state = tempfile::tempdir()?;
        let binding = config
            .connection(&ConnectionId::parse(CONNECTION_ID)?)
            .ok_or("connection")?
            .clone();
        let guest_image = write_guest_image(state.path(), &binding, &images)?;
        let set = EnsuredSet {
            id: 7,
            ownership: SetOwnership::Created,
            name: binding.scale_set.name().to_owned(),
            runner_group_id: 1,
            disable_update: false,
            labels: binding.scale_set.labels().to_vec(),
        };
        let prepared = PreparedConnection {
            binding,
            scope: scope.clone(),
            set_id: 7,
            setup: PreparedSetup {
                set,
                groups: Vec::new(),
                captured_at_unix_ms: 0,
            },
            session_intent: 0,
            assigned_total: None,
            poll_cursor: 0,
            poll_retry_at: None,
            restore_retry_at: None,
            guest_image,
            completion: None,
            credentials: None,
            runner_ready: RunnerReadyRegistry::new(),
            ready_intents: BTreeMap::new(),
            authorizing: BTreeMap::new(),
            pending_bootstrap: BTreeMap::new(),
            unkeyed_bootstrap: Vec::new(),
            authorized: BTreeMap::new(),
            pending_authorization: BTreeMap::new(),
            unkeyed_authorization: Vec::new(),
            pending_acquired_recovery: BTreeMap::new(),
            unkeyed_acquired_recovery: Vec::new(),
            admin_refresh: AdminRefreshSchedule::due_at(Instant::now()),
            recovery_pass: 0,
        };
        Ok(Self {
            config,
            images,
            image_authority: Arc::new(ImageAuthority::parse("token")?),
            state,
            prepared,
        })
    }

    pub(super) async fn turn(
        &mut self,
        journal: &Journal,
        docker: &Docker,
        engine: &SelectedEngineContext,
    ) -> TestResult<()> {
        let context = PositivePollContext {
            config: &self.config,
            journal,
            docker,
            selected_engine: engine,
            worker_images: &self.images,
            state: self.state.path(),
            image_authority: &self.image_authority,
            admission_ready: true,
        };
        super::super::reconcile_assigned_population(&mut self.prepared, &context).await?;
        Ok(())
    }
}
