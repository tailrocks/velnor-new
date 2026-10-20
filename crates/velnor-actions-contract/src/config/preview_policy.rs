//! Cross-policy authorization and producer qualification.

use super::{VelnorConfig, WorkflowPolicy};
use crate::WORKFLOW_DISPLAY_NAME;
use crate::errors::ContractError;

impl VelnorConfig {
    pub(super) fn check_binary_release_authority(&self, file: &str) -> Result<(), ContractError> {
        let enabled = self
            .stacks
            .rust
            .as_ref()
            .is_some_and(|rust| rust.binary_release.enabled());
        if !enabled {
            return Ok(());
        }
        if self.workflow.policy != WorkflowPolicy::ConsumerV1 {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release",
                "binary_release_requires_consumer_policy",
            ));
        }
        if self.workflow.name != WORKFLOW_DISPLAY_NAME {
            return Err(ContractError::config(
                file,
                "workflow.name",
                format!("native_binary_release_requires_canonical_ci_name:{WORKFLOW_DISPLAY_NAME}"),
            ));
        }
        if let Some(rust) = self
            .stacks
            .rust
            .as_ref()
            .filter(|rust| rust.binary_release.stable.enabled)
        {
            let binary = &rust.binary_release;
            let release = &rust.release;
            if !release.enabled
                || release.registry != super::ReleaseRegistry::GitTag
                || release.packages.as_slice() != [binary.version_package.as_str()]
                || release.manifest_path != binary.manifest_path
                || release.environment != binary.stable.environment
                || !super::release_validation::is_stable_tag_name(&release.tag_name)
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.binary_release.stable",
                    "requires_matching_manual_git_tag_release",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_crate_build_jobs(&self, file: &str) -> Result<(), ContractError> {
        let Some(rust) = &self.stacks.rust else {
            return Ok(());
        };
        for (package, options) in &rust.crates {
            if options.build_jobs > self.resources.compiler_process_budget {
                return Err(ContractError::config(
                    file,
                    format!("stacks.rust.crates.{package}.build_jobs"),
                    "exceeds_compiler_process_budget",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_binary_release_tools(&self, file: &str) -> Result<(), ContractError> {
        let Some(preview) = self
            .stacks
            .rust
            .as_ref()
            .map(|rust| &rust.binary_release)
            .filter(|release| release.enabled())
        else {
            return Ok(());
        };
        if u32::from(preview.producer.build_jobs) > self.resources.compiler_process_budget {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release.producer.build_jobs",
                "exceeds_compiler_process_budget",
            ));
        }
        for id in &preview.producer.tools {
            let tool = self
                .qualified_tools
                .iter()
                .find(|tool| &tool.id == id)
                .ok_or_else(|| {
                    ContractError::config(
                        file,
                        "stacks.rust.binary_release.producer.tools",
                        format!("unknown_qualified_tool:{id}"),
                    )
                })?;
            if !tool
                .platforms
                .iter()
                .any(|platform| platform.platform.target() == preview.producer.runner.mise_target())
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.binary_release.producer.tools",
                    format!("qualified_tool_missing_producer_platform:{id}"),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_distribution_source(&self, file: &str) -> Result<(), ContractError> {
        let Some(tap) = self
            .distribution
            .homebrew_tap
            .as_ref()
            .filter(|tap| tap.enabled)
        else {
            return Ok(());
        };
        for (channel, _) in tap.destinations.iter() {
            if !self
                .stacks
                .rust
                .as_ref()
                .is_some_and(|rust| rust.binary_release.channel_enabled(channel))
            {
                return Err(ContractError::config(
                    file,
                    "distribution.homebrew_tap.destinations",
                    "requires_enabled_binary_channel",
                ));
            }
        }
        Ok(())
    }
}
