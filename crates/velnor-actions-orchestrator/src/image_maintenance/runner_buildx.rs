// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{RunnerImageBuildExecution, build::support, validate};
use crate::{OrchestratorError, internal::internal};
use std::path::{Path, PathBuf};
use velnor_actions_mise::{CheckDeadline, ImageCandidateCommand};

pub(super) struct Buildx {
    pub config: PathBuf,
    plugin: PathBuf,
    sha256: String,
}

impl Buildx {
    pub(super) fn load(
        root: &Path,
        temporary: &Path,
        deadline: CheckDeadline,
        execution: &dyn RunnerImageBuildExecution,
    ) -> Result<Self, OrchestratorError> {
        let temporary = support::owned_directory(temporary)?;
        if temporary.starts_with(root) {
            return Err(internal("runner_buildx_temp"));
        }
        let config = temporary.join(".velnor-job-image-candidate/docker-config");
        let plugin = config.join("cli-plugins/docker-buildx");
        let lock = validate::read_lock(root)?;
        let result = Self {
            config,
            plugin,
            sha256: lock.sha256,
        };
        result.revalidate()?;
        let version = ImageCandidateCommand::buildx_version(&result.config)
            .and_then(|command| execution.run(&command, deadline))
            .map_err(|_| internal("runner_buildx_version"))?;
        let text = version
            .stdout_text("docker")
            .map_err(|_| internal("runner_buildx_version"))?;
        if validate::buildx_release_version(text.trim()) != Some(lock.version.as_str()) {
            return Err(internal("runner_buildx_version"));
        }
        result.verify_plugin(&lock.version, deadline, execution)?;
        result.revalidate()?;
        Ok(result)
    }

    pub(super) fn revalidate(&self) -> Result<(), OrchestratorError> {
        if support::hash_regular(&self.plugin, support::MAX_BUILDX_BYTES)? != self.sha256 {
            return Err(internal("runner_buildx_changed"));
        }
        Ok(())
    }

    fn verify_plugin(
        &self,
        version: &str,
        deadline: CheckDeadline,
        execution: &dyn RunnerImageBuildExecution,
    ) -> Result<(), OrchestratorError> {
        let output = ImageCandidateCommand::buildx_plugin_info(&self.config)
            .and_then(|command| execution.run(&command, deadline))
            .map_err(|_| internal("runner_buildx_plugin_info"))?;
        let value = velnor_actions_contract::parse_strict_json(
            &output
                .stdout_text("docker")
                .map_err(|_| internal("runner_buildx_plugin_info"))?,
        )
        .map_err(|_| internal("runner_buildx_plugin_info"))?;
        let entries = value
            .as_array()
            .ok_or_else(|| internal("runner_buildx_plugin_info"))?;
        let plugins = entries
            .iter()
            .filter(|entry| entry.get("Name").and_then(serde_json::Value::as_str) == Some("buildx"))
            .collect::<Vec<_>>();
        if plugins.len() != 1
            || plugins[0].get("Path").and_then(serde_json::Value::as_str) != self.plugin.to_str()
            || plugins[0]
                .get("Version")
                .and_then(serde_json::Value::as_str)
                != Some(format!("v{version}").as_str())
        {
            return Err(internal("runner_buildx_plugin_identity"));
        }
        Ok(())
    }
}
