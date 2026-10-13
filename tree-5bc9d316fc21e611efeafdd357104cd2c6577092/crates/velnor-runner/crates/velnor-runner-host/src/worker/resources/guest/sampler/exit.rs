//! Bounded, identity-checked probe exit polling.

use std::collections::HashMap;

use tokio::time::Instant as TokioInstant;

use super::identity::require_owned_id;
use super::{GuestDockerClient, GuestSampleFailure, GuestSampler};

impl<C: GuestDockerClient> GuestSampler<'_, C> {
    /// Wait for this exact owned probe to exit within both deadlines.
    pub(super) async fn wait_for_exit(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
        expected_id: &str,
        deadline: TokioInstant,
    ) -> Result<i64, GuestSampleFailure> {
        loop {
            if TokioInstant::now() >= deadline {
                return Err(GuestSampleFailure::Timeout);
            }
            self.verify_engine(deadline).await?;
            let container = self
                .call(deadline, self.client.inspect_container(name))
                .await?
                .ok_or(GuestSampleFailure::ProbeExit)?;
            let found_id = require_owned_id(&container, name, labels)?;
            if found_id != expected_id {
                return Err(GuestSampleFailure::Ownership);
            }
            self.verify_engine(deadline).await?;
            if TokioInstant::now() >= deadline {
                return Err(GuestSampleFailure::Timeout);
            }
            match (container.running, container.exit_code) {
                (Some(false), Some(code)) => return Ok(code),
                (Some(true), _) => {
                    let remaining = deadline.saturating_duration_since(TokioInstant::now());
                    tokio::time::sleep(self.timing.exit_poll.min(remaining)).await;
                }
                _ => return Err(GuestSampleFailure::ProbeExit),
            }
        }
    }
}
