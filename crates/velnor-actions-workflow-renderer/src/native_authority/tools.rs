// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, NativeAuthorityInput, invalid};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_mise::catalog::{
    JACTIONLINT_DIGESTS, OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64,
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeToolOrigin {
    ProjectDeclaration,
    ConfigOverride,
}
#[derive(Debug, Clone)]
pub struct NativeToolPin {
    pub tool: PinnedTool,
    pub version: String,
    pub origin: NativeToolOrigin,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRustSource {
    ToolchainFile,
    ConfigOrDefault,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRustProvider {
    Rustup,
    MrBoxington,
}

pub(super) fn catalog(
    input: &NativeAuthorityInput<'_>,
) -> Result<ToolCatalog, NativeAuthorityError> {
    let defaults = ToolCatalog::pinned();
    let mut selected = BTreeMap::new();
    let mut project = BTreeSet::new();
    for pin in input.tool_pins {
        velnor_actions_mise::catalog::validate_concrete_exact_version(
            pin.tool.tool_name(),
            &pin.version,
        )
        .map_err(NativeAuthorityError::Tool)?;
        if pin.tool == PinnedTool::Gh && pin.version != defaults.version(PinnedTool::Gh) {
            return Err(invalid("native_gh_credential_pin"));
        }
        if selected.insert(pin.tool, pin.version.as_str()).is_some() {
            return Err(invalid("native_tool_duplicate"));
        }
        let configured = configured(input.config, pin.tool);
        match pin.origin {
            NativeToolOrigin::ConfigOverride if configured != Some(pin.version.as_str()) => {
                return Err(invalid("native_tool_config_mismatch"));
            }
            NativeToolOrigin::ProjectDeclaration => {
                if pin.tool == PinnedTool::Gh {
                    return Err(invalid("native_gh_credential_pin"));
                }
                if configured.is_some_and(|version| version != pin.version) {
                    return Err(invalid("native_tool_project_conflict"));
                }
                project.insert(pin.tool);
            }
            NativeToolOrigin::ConfigOverride => {}
        }
    }
    for tool in &PinnedTool::ALL {
        if configured(input.config, *tool)
            .is_some_and(|version| selected.get(tool).copied() != Some(version))
        {
            return Err(invalid("native_tool_config_missing"));
        }
    }
    if input
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.toolchain_source.is_some())
        && !project.contains(&PinnedTool::Rust)
    {
        return Err(invalid("native_rust_source_pin"));
    }
    if input.rust_provider == NativeRustProvider::MrBoxington
        && !project.contains(&PinnedTool::MrBoxington)
    {
        return Err(invalid("native_rust_provider_pin"));
    }
    let version = |tool| {
        selected
            .get(&tool)
            .copied()
            .unwrap_or_else(|| defaults.version(tool))
    };
    let mut catalog = ToolCatalog::new(
        version(PinnedTool::Rust),
        version(PinnedTool::MrBoxington),
        version(PinnedTool::Gh),
        version(PinnedTool::Jactionlint),
        version(PinnedTool::Shellcheck),
        version(PinnedTool::Nextest),
        version(PinnedTool::Opentofu),
    )
    .map_err(NativeAuthorityError::Tool)?;
    for tool in project {
        catalog = catalog
            .with_project_tool(tool, version(tool))
            .map_err(NativeAuthorityError::Tool)?;
    }
    catalog = catalog
        .with_export_rustup_toolchain(input.rust_source == NativeRustSource::ConfigOrDefault)
        .with_rust_mr_boxington(input.rust_provider == NativeRustProvider::MrBoxington);
    Ok(catalog.for_emission())
}

fn configured(config: &VelnorConfig, tool: PinnedTool) -> Option<&str> {
    if tool == PinnedTool::Rust {
        config
            .stacks
            .rust
            .as_ref()
            .and_then(|rust| rust.toolchain.as_deref())
            .or_else(|| config.tools.get("rust"))
    } else if tool == PinnedTool::Nextest {
        config.tools.get("cargo-nextest")
    } else {
        config.tools.get(tool.tool_name())
    }
}

pub(super) fn mise_setup(
    config: &VelnorConfig,
    target: ReleaseTarget,
) -> Result<crate::MiseSetup, NativeAuthorityError> {
    use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MISE_ACTION_VERSION};
    use velnor_actions_actionlint::overrides::{ActionPinOverride, ApprovedPinCatalog};
    let action = "jdx/mise-action";
    let uses = if let Some(pin) = config.actions.overrides.get(action) {
        let mut catalog = ApprovedPinCatalog::new();
        catalog
            .insert(action, MISE_ACTION_SHA, MISE_ACTION_VERSION)
            .map_err(|error| invalid(error.to_string()))?;
        let approved = catalog
            .validate_override(&ActionPinOverride {
                action: action.to_owned(),
                sha: pin.sha.clone(),
                version: pin.version.clone(),
            })
            .map_err(|error| invalid(error.to_string()))?;
        format!("{}@{}", approved.uses_key(), pin.sha)
    } else {
        format!("{action}@{MISE_ACTION_SHA}")
    };
    let sha256 = match target {
        ReleaseTarget::LinuxX86_64 => crate::setup::MISE_BINARY_SHA256_LINUX_X64,
        ReleaseTarget::MacosArm64 => crate::setup::MISE_BINARY_SHA256_MACOS_ARM64,
        ReleaseTarget::MacosX86_64 => crate::setup::MISE_BINARY_SHA256_MACOS_X64,
    };
    Ok(crate::MiseSetup {
        uses,
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: sha256.to_owned(),
    })
}

#[must_use]
pub fn catalog_tool_pins(catalog: &ToolCatalog) -> Vec<String> {
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
