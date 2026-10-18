//! Cross-policy authorization and producer qualification.

use super::VelnorConfig;
use crate::errors::ContractError;

impl VelnorConfig {
    pub(super) fn check_preview_tools(&self, file: &str) -> Result<(), ContractError> {
        let Some(preview) = self
            .stacks
            .rust
            .as_ref()
            .map(|rust| &rust.binary_release.preview)
            .filter(|preview| preview.enabled)
        else {
            return Ok(());
        };
        if u32::from(preview.producer.build_jobs) > self.resources.compiler_process_budget {
            return Err(ContractError::config(
                file,
                "stacks.rust.binary_release.preview.producer.build_jobs",
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
                        "stacks.rust.binary_release.preview.producer.tools",
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
                    "stacks.rust.binary_release.preview.producer.tools",
                    format!("qualified_tool_missing_producer_platform:{id}"),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_distribution_source(&self, file: &str) -> Result<(), ContractError> {
        if self
            .distribution
            .homebrew_tap
            .as_ref()
            .is_some_and(|tap| tap.enabled)
            && !self
                .stacks
                .rust
                .as_ref()
                .is_some_and(|rust| rust.binary_release.preview.enabled)
        {
            return Err(ContractError::config(
                file,
                "distribution.homebrew_tap.source",
                "requires_enabled_binary_preview",
            ));
        }
        Ok(())
    }
}
