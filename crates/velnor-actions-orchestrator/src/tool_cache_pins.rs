//! Catalog archive pins the tools-cache restore re-verifies restored Mise
//! installs against when `mise.lock` has no entry for a tool.
//!
//! One line per `(install directory, version, Mise platform, release
//! archive URL, archive SHA-256)`, for the Velnor-pinned tools whose archive
//! digests the catalog records (jactionlint, `OpenTofu`). A project that
//! declares another version gets no pin for it, so its restored install is
//! discarded and reinstalled from `mise.lock` or the network.

use velnor_actions_mise::catalog::{
    JACTIONLINT_DIGESTS, OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64,
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

/// Pins for the catalog versions this generation emits.
pub(crate) fn catalog_tool_pins(catalog: &ToolCatalog) -> Vec<String> {
    let mut pins = Vec::new();
    let jactionlint = catalog.version(PinnedTool::Jactionlint);
    for row in JACTIONLINT_DIGESTS
        .iter()
        .filter(|row| row.version == jactionlint)
    {
        pins.push(format!(
            "jactionlint {} {} https://github.com/jdx/jactionlint/releases/download/v{}/{} {}",
            row.version, row.platform, row.version, row.asset, row.archive_sha256
        ));
    }
    if catalog.version(PinnedTool::Opentofu) == OPENTOFU_VERSION {
        for (platform, asset, sha) in [
            ("linux-x64", "linux_amd64", OPENTOFU_SHA256_LINUX_AMD64),
            ("linux-arm64", "linux_arm64", OPENTOFU_SHA256_LINUX_ARM64),
            ("macos-x64", "darwin_amd64", OPENTOFU_SHA256_DARWIN_AMD64),
            ("macos-arm64", "darwin_arm64", OPENTOFU_SHA256_DARWIN_ARM64),
        ] {
            pins.push(format!(
                "opentofu {OPENTOFU_VERSION} {platform} https://github.com/opentofu/opentofu/releases/download/v{OPENTOFU_VERSION}/tofu_{OPENTOFU_VERSION}_{asset}.tar.gz {sha}"
            ));
        }
    }
    pins
}

#[cfg(test)]
mod tests {
    use super::catalog_tool_pins;
    use velnor_actions_mise::ToolCatalog;

    #[test]
    fn every_pin_names_an_https_archive_and_a_sha256_per_platform() {
        let pins = catalog_tool_pins(&ToolCatalog::pinned());
        assert!(pins.iter().any(|pin| pin.starts_with("jactionlint ")));
        assert_eq!(
            pins.iter()
                .filter(|pin| pin.starts_with("opentofu "))
                .count(),
            4
        );
        for pin in &pins {
            let fields: Vec<&str> = pin.split(' ').collect();
            assert_eq!(fields.len(), 5, "{pin}");
            assert!(fields[3].starts_with("https://github.com/"), "{pin}");
            assert_eq!(fields[4].len(), 64, "{pin}");
        }
    }
}
