// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_archive_guard::owned_archive_guard::oci_candidate::OciImageConfiguration;

use crate::{OrchestratorError, internal::internal};

const PROBE_DEFAULT_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Clone, Copy)]
pub(super) enum ImageKind {
    Runner,
    Dind,
    ResourceProbe,
}

impl ImageKind {
    pub(super) const fn repository(self) -> &'static str {
        match self {
            Self::Runner => "ghcr.io/tailrocks/velnor-runner",
            Self::Dind => "ghcr.io/tailrocks/velnor-dind",
            Self::ResourceProbe => "ghcr.io/tailrocks/velnor-resource-probe",
        }
    }
}

pub(super) fn validate_revision(
    configuration: &OciImageConfiguration,
    source_commit: &str,
) -> Result<(), OrchestratorError> {
    if configuration
        .labels
        .as_ref()
        .and_then(|labels| labels.get("org.opencontainers.image.revision"))
        .is_none_or(|revision| revision != source_commit)
    {
        return Err(internal("runner_image_revision"));
    }
    Ok(())
}

pub(super) fn validate_probe_profile(
    configuration: &OciImageConfiguration,
) -> Result<(), OrchestratorError> {
    if configuration.config_keys.len() != 5
        || !["User", "Env", "Entrypoint", "WorkingDir", "Labels"]
            .iter()
            .all(|key| configuration.config_keys.contains(*key))
        || configuration.labels.as_ref().is_none_or(|labels| {
            labels.len() != 1 || !labels.contains_key("org.opencontainers.image.revision")
        })
        || configuration.user.as_deref() != Some("65532:65532")
        || configuration.working_directory.as_deref() != Some("/")
        || configuration.entrypoint.as_ref().is_none_or(|entrypoint| {
            entrypoint.len() != 1 || entrypoint[0] != "/velnor/resource-probe"
        })
        || configuration.cmd.is_some()
        || configuration.env.as_ref().is_none_or(|environment| {
            environment.len() != 1 || environment[0] != PROBE_DEFAULT_ENV
        })
    {
        return Err(internal("runner_image_probe_profile"));
    }
    Ok(())
}
