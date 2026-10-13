//! Bounded, label-checked reconciliation and cleanup for probe containers.

use std::collections::HashMap;
use tokio::time::Instant as TokioInstant;

use super::identity::{container_name, owner_labels, require_owned_container_id, require_owned_id};
use super::{GuestDockerClient, GuestSampleFailure, GuestSampler};

impl<C: GuestDockerClient> GuestSampler<'_, C> {
    /// Reconcile a known ID, an uncertain create, or a stale reserved name.
    pub(super) async fn cleanup_probe(
        &self,
        expected_id: Option<&str>,
        expected_image: Option<&str>,
        create_uncertain: bool,
        deadline: TokioInstant,
    ) -> Result<(), GuestSampleFailure> {
        self.verify_engine(deadline).await?;
        let name = container_name(self.identity);
        let labels = owner_labels(self.identity);
        if let Some(id) = expected_id {
            return self
                .remove_owned(id, &labels, expected_image, deadline)
                .await;
        }
        if create_uncertain {
            return self
                .reconcile_late_create(&name, &labels, expected_image, deadline)
                .await;
        }
        match self
            .cleanup_call(deadline, self.client.inspect_container(&name))
            .await?
        {
            None => Ok(()),
            Some(container) => {
                let id = require_owned_id(&container, &name, &labels)?;
                ensure_expected_image(&container, expected_image)?;
                self.remove_owned(&id, &labels, expected_image, deadline)
                    .await
            }
        }
    }

    async fn reconcile_late_create(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
        expected_image: Option<&str>,
        deadline: TokioInstant,
    ) -> Result<(), GuestSampleFailure> {
        loop {
            self.verify_engine(deadline).await?;
            match self
                .cleanup_call(deadline, self.client.inspect_container(name))
                .await
            {
                Ok(Some(container)) => {
                    let id = require_owned_id(&container, name, labels)?;
                    ensure_expected_image(&container, expected_image)?;
                    return self
                        .remove_owned(&id, labels, expected_image, deadline)
                        .await;
                }
                Ok(None) | Err(GuestSampleFailure::Docker | GuestSampleFailure::Timeout) => {}
                Err(reason) => return Err(reason),
            }
            let remaining = deadline.saturating_duration_since(TokioInstant::now());
            if remaining.is_zero() {
                return Err(GuestSampleFailure::CreateUncertain);
            }
            tokio::time::sleep(self.timing.exit_poll.min(remaining)).await;
        }
    }

    pub(super) async fn remove_owned(
        &self,
        id: &str,
        labels: &HashMap<String, String>,
        expected_image: Option<&str>,
        deadline: TokioInstant,
    ) -> Result<(), GuestSampleFailure> {
        self.verify_engine(deadline).await?;
        match self
            .cleanup_call(deadline, self.client.inspect_container(id))
            .await?
        {
            None => return Ok(()),
            Some(container) => {
                require_owned_container_id(&container, id, labels)?;
                ensure_expected_image(&container, expected_image)?;
            }
        }
        self.verify_engine(deadline).await?;
        match self
            .cleanup_call(deadline, self.client.remove_container(id))
            .await
        {
            Ok(()) | Err(_) => {}
        }
        if self.inspect_after_remove(id, labels, deadline).await? {
            return Ok(());
        }
        self.verify_engine(deadline).await?;
        match self
            .cleanup_call(deadline, self.client.remove_container(id))
            .await
        {
            Ok(()) | Err(_) => {}
        }
        if self.inspect_after_remove(id, labels, deadline).await? {
            Ok(())
        } else {
            Err(GuestSampleFailure::Cleanup)
        }
    }

    async fn inspect_after_remove(
        &self,
        id: &str,
        labels: &HashMap<String, String>,
        deadline: TokioInstant,
    ) -> Result<bool, GuestSampleFailure> {
        self.verify_engine(deadline).await?;
        match self
            .cleanup_call(deadline, self.client.inspect_container(id))
            .await?
        {
            None => Ok(true),
            Some(container) => {
                require_owned_container_id(&container, id, labels)?;
                Ok(false)
            }
        }
    }
}

fn ensure_expected_image(
    container: &super::ProbeContainer,
    expected_image: Option<&str>,
) -> Result<(), GuestSampleFailure> {
    if expected_image.is_some_and(|image| container.image.as_deref() != Some(image)) {
        return Err(GuestSampleFailure::Ownership);
    }
    Ok(())
}
