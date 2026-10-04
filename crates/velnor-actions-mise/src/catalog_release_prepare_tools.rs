use super::{DistributionHost, RustReleaseTools};
use crate::MiseError;

/// Environment key carrying the generator-owned release-plz configuration.
pub const RELEASE_PREPARE_CONFIG: &str = "RELEASE_PREPARE_CONFIG";
/// Environment key carrying the fixed cargo-semver-checks version.
pub const RELEASE_SEMVER_CHECKS_VERSION: &str = "RELEASE_SEMVER_CHECKS_VERSION";

/// Compile the anonymous Mac release preparation tool closure.
///
/// # Errors
/// Rejects unsupported hosts and missing owned executable qualification.
pub fn rust_release_prepare_tools(
    host: DistributionHost,
    generator_version: &str,
) -> Result<RustReleaseTools, MiseError> {
    let _ = generator_version;
    let _catalog = crate::catalog::ToolCatalog::for_release_host(host)?;
    Err(MiseError::Contract {
        problem: format!(
            "qualified release preparation tools absent: {} requires owned publication and behavioral qualification",
            host.abi()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_rejected_by_release_host_authority() {
        let result = rust_release_prepare_tools(DistributionHost::LinuxAmd64, "test-generator");
        assert!(matches!(
            result,
            Err(MiseError::InvalidStepInput { field, value })
                if field == "release_rust_host"
                    && value == DistributionHost::LinuxAmd64.abi()
        ));
    }

    #[test]
    fn mac_requires_owned_publication() {
        let result = rust_release_prepare_tools(DistributionHost::MacosArm64, "test-generator");
        assert!(matches!(
            result,
            Err(MiseError::Contract { problem })
                if problem
                    == "qualified release preparation tools absent: aarch64-apple-darwin requires owned publication and behavioral qualification"
        ));
    }
}
