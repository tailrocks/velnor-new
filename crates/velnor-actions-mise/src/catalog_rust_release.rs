//! Closed root compiler authority for source-only Mac release tooling.

use super::{ToolCatalog, qualification::DistributionHost, rust_desktop::RustCompilerRole};
use crate::MiseError;

impl ToolCatalog {
    /// Fixed root Rust release tooling on a closed supported release host.
    ///
    /// # Errors
    /// Rejects hosts outside the closed Mac release authority.
    /// Preparation separately requires exact qualified artifact plans.
    pub fn for_release_host(host: DistributionHost) -> Result<Self, MiseError> {
        if host != DistributionHost::MacosArm64 {
            return Err(MiseError::InvalidStepInput {
                field: "release_rust_host".to_owned(),
                value: host.abi().to_owned(),
            });
        }
        let mut catalog = Self::pinned();
        catalog.rust_role = RustCompilerRole::ReleaseMac;
        Ok(catalog)
    }
}
