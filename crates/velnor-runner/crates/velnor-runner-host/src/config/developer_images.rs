// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::error::HostError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeveloperImages {
    runner: String,
    dind: String,
    resource_probe: String,
}

impl DeveloperImages {
    #[must_use]
    pub fn runner(&self) -> &str {
        &self.runner
    }

    #[must_use]
    pub fn dind(&self) -> &str {
        &self.dind
    }

    #[must_use]
    pub fn resource_probe(&self) -> &str {
        &self.resource_probe
    }
}

pub(super) fn validate_developer_images(
    images: Option<&DeveloperImages>,
    release_read_configured: bool,
    release_build: bool,
) -> Result<(), HostError> {
    let Some(images) = images else {
        return Ok(());
    };
    if release_build {
        return Err(HostError::DeveloperImages);
    }
    if release_read_configured
        || ![&images.runner, &images.dind, &images.resource_probe]
            .iter()
            .all(|id| image_id(id))
        || images.runner == images.dind
        || images.runner == images.resource_probe
        || images.dind == images.resource_probe
    {
        return Err(HostError::Config);
    }
    Ok(())
}

fn image_id(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests;
