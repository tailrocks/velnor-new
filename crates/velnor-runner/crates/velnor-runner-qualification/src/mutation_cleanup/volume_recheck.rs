// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::models::Volume;

use crate::error::QualificationError as HostError;

use super::{OwnedResources, deadline, missing, volume_owned};

impl OwnedResources {
    pub(super) async fn volume_unchanged(
        &mut self,
        docker: &Docker,
        name: &str,
        role: &str,
        volume: &Volume,
        identity: &str,
    ) -> Result<bool, HostError> {
        let current = match deadline(docker.inspect_volume(name), Duration::from_secs(3)).await? {
            Ok(current) => current,
            Err(error) if missing(&error) => {
                self.finish_delete(identity)?;
                return Ok(false);
            }
            Err(_) => return Err(HostError::Docker),
        };
        if !volume_owned(&current, name, &self.run_id, role)
            || current.created_at != volume.created_at
            || current.mountpoint != volume.mountpoint
        {
            self.finish_delete(identity)?;
            return Err(HostError::Ownership);
        }
        Ok(true)
    }
}
