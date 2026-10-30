// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::cachekey::{ToolchainInputs, validate_semantic_text};
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedTestTool {
    pub tool: crate::config::CrateTestTool,

    pub version: String,

    pub backend: String,

    pub platform: String,

    pub options: std::collections::BTreeMap<String, String>,

    pub checksum: String,

    pub url: String,

    pub signer: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_bindings: Vec<SelectedTestToolSourceBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedTestToolSourceBinding {
    pub path: String,

    pub sha256: String,
}

pub fn toolchain_id(inputs: &ToolchainInputs) -> Result<String, ContractError> {
    if inputs.tools.is_empty() {
        return Err(ContractError::identity("toolchain", "no_pinned_tools"));
    }
    let mut tools = inputs.tools.clone();
    tools.sort();
    if inputs.tools != tools {
        return Err(ContractError::identity("toolchain", "tools_must_be_sorted"));
    }
    for tool in &inputs.tools {
        validate_semantic_text("toolchain.tool", tool)?;
    }
    validate_semantic_text("compile_driver", &inputs.compile_driver)?;
    validate_semantic_text("test_runner", &inputs.test_runner)?;
    if inputs.selected_test_tools.is_empty() {
        return Ok(digest_b3(&canonical_json_bytes(inputs)?));
    }
    validate_selected_test_tools(&inputs.selected_test_tools)?;
    let identity = ToolchainIdentityInputs {
        tools: &inputs.tools,
        components: &inputs.components,
        compile_driver: &inputs.compile_driver,
        test_runner: &inputs.test_runner,
        selected_test_tools: inputs
            .selected_test_tools
            .iter()
            .map(SelectedTestTool::identity)
            .collect(),
    };
    Ok(digest_b3(&canonical_json_bytes(&identity)?))
}

#[derive(Serialize)]
struct ToolchainIdentityInputs<'a> {
    tools: &'a [String],
    components: &'a [String],
    compile_driver: &'a str,
    test_runner: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    selected_test_tools: Vec<SelectedTestToolIdentity<'a>>,
}

#[derive(Serialize)]
struct SelectedTestToolIdentity<'a> {
    tool: crate::config::CrateTestTool,
    version: &'a str,
    backend: &'a str,
    platform: &'a str,
    options: &'a std::collections::BTreeMap<String, String>,
    checksum: &'a str,
    url: &'a str,
    signer: &'a str,
}

impl SelectedTestTool {
    fn identity(&self) -> SelectedTestToolIdentity<'_> {
        SelectedTestToolIdentity {
            tool: self.tool,
            version: &self.version,
            backend: &self.backend,
            platform: &self.platform,
            options: &self.options,
            checksum: &self.checksum,
            url: &self.url,
            signer: &self.signer,
        }
    }
}

fn validate_selected_test_tools(tools: &[SelectedTestTool]) -> Result<(), ContractError> {
    if tools.windows(2).any(|pair| pair[0].tool >= pair[1].tool) {
        return Err(ContractError::identity(
            "toolchain.selected_test_tools",
            "tools_must_be_sorted_and_unique",
        ));
    }
    for tool in tools {
        if tool.tool != crate::config::CrateTestTool::Hk
            || !crate::config::is_exact_semver(&tool.version)
            || tool.backend != "packslip:github.com/jdx/hk"
            || !matches!(tool.platform.as_str(), "linux-x64" | "linux-arm64")
            || !tool.options.is_empty()
            || tool.signer
                != "sigstore-oidc:https://github.com/jdx/hk/.github/workflows/release.yml"
        {
            return Err(ContractError::identity(
                "toolchain.selected_test_tools",
                "unsupported_selected_test_tool",
            ));
        }
        let asset = match tool.platform.as_str() {
            "linux-x64" => "hk-x86_64-unknown-linux-gnu.tar.gz",
            "linux-arm64" => "hk-aarch64-unknown-linux-gnu.tar.gz",
            _ => {
                return Err(ContractError::identity(
                    "toolchain.selected_test_tools.platform",
                    "unsupported_selected_test_tool_platform",
                ));
            }
        };
        let expected_url = format!(
            "https://github.com/jdx/hk/releases/download/v{}/{asset}",
            tool.version
        );
        if tool.url != expected_url || !is_sha256_checksum(&tool.checksum) {
            return Err(ContractError::identity(
                "toolchain.selected_test_tools",
                "invalid_selected_test_tool_artifact",
            ));
        }
        let mut bindings = tool.source_bindings.clone();
        bindings.sort_by(|left, right| left.path.cmp(&right.path));
        if bindings != tool.source_bindings
            || bindings.windows(2).any(|pair| pair[0].path == pair[1].path)
            || bindings.is_empty()
        {
            return Err(ContractError::identity(
                "toolchain.selected_test_tools.source_bindings",
                "bindings_must_be_sorted_unique_and_present",
            ));
        }
        for binding in &tool.source_bindings {
            let normalized = crate::canonical::normalize_posix_path(&binding.path)?;
            if normalized != binding.path || !is_sha256_hex(&binding.sha256) {
                return Err(ContractError::identity(
                    "toolchain.selected_test_tools.source_bindings",
                    "invalid_source_binding",
                ));
            }
        }
    }
    Ok(())
}

fn is_sha256_checksum(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_sha256_hex)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
