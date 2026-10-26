// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::catalog::{
    JACTIONLINT_DIGESTS, OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64,
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

pub(crate) const TOOL_PIN_HOSTS: [&str; 4] =
    ["linux-x64", "linux-arm64", "macos-x64", "macos-arm64"];

pub(crate) fn catalog_tool_pins_for_host(catalog: &ToolCatalog, host: &str) -> Vec<String> {
    if !TOOL_PIN_HOSTS.contains(&host) {
        return Vec::new();
    }
    let mut pins = Vec::new();
    let jactionlint = catalog.version(PinnedTool::Jactionlint);
    for row in JACTIONLINT_DIGESTS
        .iter()
        .filter(|row| row.version == jactionlint && row.platform == host)
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
            if platform == host {
                pins.push(format!(
                    "opentofu {OPENTOFU_VERSION} {platform} https://github.com/opentofu/opentofu/releases/download/v{OPENTOFU_VERSION}/tofu_{OPENTOFU_VERSION}_{asset}.tar.gz {sha}"
                ));
            }
        }
    }
    union_pins(pins)
}

pub(crate) fn catalog_tool_pins(catalog: &ToolCatalog) -> Vec<String> {
    let mut pins = Vec::new();
    for host in TOOL_PIN_HOSTS {
        pins.extend(catalog_tool_pins_for_host(catalog, host));
    }
    union_pins(pins)
}

fn union_pins(pins: Vec<String>) -> Vec<String> {
    let mut union = Vec::with_capacity(pins.len());
    for pin in pins {
        if !union.contains(&pin) {
            union.push(pin);
        }
    }
    union
}
