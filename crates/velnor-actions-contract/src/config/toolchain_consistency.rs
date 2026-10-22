//! Cross-check repository-wide tool declarations.

use super::VelnorConfig;
use crate::errors::ContractError;

impl VelnorConfig {
    pub(super) fn check_toolchain_consistency(&self, file: &str) -> Result<(), ContractError> {
        if let (Some(stack_tc), Some(tools_tc)) = (
            self.stacks
                .rust
                .as_ref()
                .and_then(|rust| rust.toolchain.as_deref()),
            self.tools.get("rust"),
        ) && stack_tc != tools_tc
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.toolchain",
                format!(
                    "conflicting_rust_toolchain:stacks.rust.toolchain={stack_tc},tools.rust={tools_tc}"
                ),
            ));
        }
        Ok(())
    }
}
