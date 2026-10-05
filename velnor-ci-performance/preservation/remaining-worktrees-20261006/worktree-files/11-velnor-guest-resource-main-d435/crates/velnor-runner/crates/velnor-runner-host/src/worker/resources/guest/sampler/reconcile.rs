//! Reconcile timed-out creation and confirm the selected daemon.

use std::collections::HashMap;

use bollard::models::SystemInfo;
use tokio::time::{Instant as TokioInstant, sleep};

use super::identity::require_owned_id;
use super::{GuestDockerClient, GuestSampleFailure, GuestSampler};

impl<C: GuestDockerClient> GuestSampler<'_, C> {
    /// Find a late create by its reserved name and exact ownership labels.
    pub(super) async fn reconcile_create(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
        image_id: &str,
        work_deadline: TokioInstant,
        owned_id: &mut Option<String>,
    ) -> Result<(), GuestSampleFailure> {
        let deadline = work_deadline.min(TokioInstant::now() + self.timing.create_reconcile);
        loop {
            self.verify_engine(deadline).await?;
            match self
                .call(deadline, self.client.inspect_container(name))
                .await
            {
                Ok(Some(container)) => {
                    let id = require_owned_id(&container, name, labels)?;
                    if container.image.as_deref() != Some(image_id) {
                        return Err(GuestSampleFailure::Ownership);
                    }
                    *owned_id = Some(id);
                    return Ok(());
                }
                Ok(None) | Err(GuestSampleFailure::Docker | GuestSampleFailure::Timeout) => {}
                Err(reason) => return Err(reason),
            }
            let remaining = deadline.saturating_duration_since(TokioInstant::now());
            if remaining.is_zero() {
                return Err(GuestSampleFailure::CreateUncertain);
            }
            sleep(self.timing.exit_poll.min(remaining)).await;
        }
    }

    /// Verify that this Docker client still selects the paired engine.
    pub(super) async fn verify_engine(
        &self,
        deadline: TokioInstant,
    ) -> Result<SystemInfo, GuestSampleFailure> {
        let info = self.call(deadline, self.client.info()).await?;
        if info.id.as_deref() == Some(self.identity.engine_id()) {
            Ok(info)
        } else {
            Err(GuestSampleFailure::EngineIdentity)
        }
    }
}
