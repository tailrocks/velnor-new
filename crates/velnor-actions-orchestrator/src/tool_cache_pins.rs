use velnor_actions_mise::catalog::{
    JACTIONLINT_DIGESTS, OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64,
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

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
