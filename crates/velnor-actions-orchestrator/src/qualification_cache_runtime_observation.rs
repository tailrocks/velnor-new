//! Measure pinned executables and runtime ABI facts for a cache owner.

use std::path::Path;

use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::workflow::{
    QualificationRuntimeAbiObservation as Abi, QualificationRuntimeIdentity as Identity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform as Platform,
    QualificationRuntimeToolObservation as Tool,
};
use velnor_actions_mise::requests::MiseExecutableLookup;

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "qualification_cache_runtime_observation/probe.rs"]
mod probe;
use probe::{ensure_same_executable, probe, run_lookup};

pub(super) fn observe(
    platform: Platform,
    requirements: &QualificationRuntimeIdentityRequirements,
    mise_data: &Path,
    rustup_home: &Path,
) -> Result<Identity, OrchestratorError> {
    let inputs = &requirements.toolchain_inputs;
    let mut tools = Vec::with_capacity(inputs.tools.len());
    let mut rust = None;
    let mut java = None;
    let mut node = None;
    let mut tofu = None;
    for selector in &inputs.tools {
        let observed = observe_tool(selector, &requirements.target, mise_data, rustup_home)?;
        tools.push(observed.tool);
        match observed.abi {
            Some(fact @ Abi::Rust { .. }) => rust = Some(fact),
            Some(fact @ Abi::Java { .. }) => java = Some(fact),
            Some(fact @ Abi::Node { .. }) => node = Some(fact),
            Some(fact @ Abi::OpenTofu { .. }) => tofu = Some(fact),
            Some(Abi::Host { .. }) | None => {}
        }
    }
    tools.sort_by(|left, right| left.selector.cmp(&right.selector));
    let mut abi = vec![observe_host(inputs, &requirements.target)?];
    if let Some(fact) = rust {
        abi.push(fact);
    }
    if let Some(fact) = java {
        abi.push(fact);
    }
    if let Some(fact) = node {
        abi.push(fact);
    }
    if let Some(fact) = tofu {
        abi.push(fact);
    }
    Ok(Identity {
        platform,
        tools,
        abi,
    })
}

struct ObservedTool {
    tool: Tool,
    abi: Option<Abi>,
}

fn observe_tool(
    selector: &str,
    target: &str,
    mise_data: &Path,
    rustup_home: &Path,
) -> Result<ObservedTool, OrchestratorError> {
    let (name, version) = selector
        .rsplit_once('@')
        .ok_or_else(|| internal("qualification_runtime_selector_invalid"))?;
    let executable = executable_for(name)?;
    let install_root = if name == "rust" {
        rustup_home
    } else {
        mise_data
    };
    let lookup = if name == "rust" {
        MiseExecutableLookup::rust_toolchain(selector, "rustc")
    } else {
        MiseExecutableLookup::mise(selector, executable)
    }
    .map_err(|_| internal("qualification_runtime_executable_lookup"))?;
    let (observed, version_output) =
        run_lookup(&lookup, selector, executable, install_root, &["--version"])?;
    if !contains_version(&version_output, version) {
        return Err(internal("qualification_runtime_version_mismatch"));
    }
    let tool = Tool {
        selector: selector.to_owned(),
        reported_version: version.to_owned(),
        executable_sha256: observed.sha256.clone(),
    };
    let abi = match name {
        "rust" => Some(observe_rust(
            selector,
            executable,
            target,
            &lookup,
            rustup_home,
            &observed.sha256,
        )?),
        "java" => Some(observe_java(
            selector,
            executable,
            &lookup,
            mise_data,
            &observed.sha256,
        )?),
        "node" | "nodejs" | "bun" => Some(observe_node(
            selector,
            executable,
            &lookup,
            mise_data,
            &observed.sha256,
            &version_output,
        )?),
        "opentofu" => Some(observe_tofu(
            selector,
            executable,
            &lookup,
            mise_data,
            &observed.sha256,
        )?),
        _ => None,
    };
    Ok(ObservedTool { tool, abi })
}

fn executable_for(name: &str) -> Result<&'static str, OrchestratorError> {
    match name {
        "rust" => Ok("rustc"),
        "java" => Ok("java"),
        "node" | "nodejs" => Ok("node"),
        "bun" => Ok("bun"),
        "opentofu" => Ok("tofu"),
        "mr-boxington" => Ok("mbx"),
        "gh" => Ok("gh"),
        "jactionlint" => Ok("jactionlint"),
        "shellcheck" => Ok("shellcheck"),
        "release-plz" => Ok("release-plz"),
        "aqua:nextest-rs/nextest/cargo-nextest" => Ok("cargo-nextest"),
        _ => Err(internal("qualification_runtime_selector_unsupported")),
    }
}

fn observe_rust(
    selector: &str,
    executable: &str,
    target: &str,
    lookup: &MiseExecutableLookup,
    root: &Path,
    expected_sha: &str,
) -> Result<Abi, OrchestratorError> {
    let (observed, verbose) = run_lookup(
        lookup,
        selector,
        executable,
        root,
        &["--version", "--verbose"],
    )?;
    ensure_same_executable(&observed.sha256, expected_sha)?;
    let version = selector
        .strip_prefix("rust@")
        .ok_or_else(|| internal("qualification_runtime_selector_invalid"))?;
    let host = prefixed_line(&verbose, "host: ")?;
    let (observed, cfg) = run_lookup(
        lookup,
        selector,
        executable,
        root,
        &["--print", "cfg", "--target", target],
    )?;
    ensure_same_executable(&observed.sha256, expected_sha)?;
    let installed = probe(
        selector,
        &[
            "rustup",
            "component",
            "list",
            "--toolchain",
            version,
            "--installed",
        ],
    )?;
    let mut components = installed
        .lines()
        .filter_map(|line| inputs_component(line).map(str::to_owned))
        .collect::<Vec<_>>();
    components.sort();
    components.dedup();
    if !contains_version(&verbose, version) || components.is_empty() {
        return Err(internal("qualification_runtime_rust_observation_invalid"));
    }
    Ok(Abi::Rust {
        host,
        target: target.to_owned(),
        version_verbose: verbose,
        target_cfg: cfg,
        installed_components: components,
    })
}

fn inputs_component(line: &str) -> Option<&str> {
    let (name, state) = line.split_once(' ')?;
    if state != "(installed)" {
        return None;
    }
    ["clippy", "rustfmt", "rustc", "cargo"]
        .into_iter()
        .find(|component| name == *component || name.starts_with(&format!("{component}-")))
}

fn prefixed_line(text: &str, prefix: &str) -> Result<String, OrchestratorError> {
    let mut found = text.lines().filter_map(|line| line.strip_prefix(prefix));
    let value = found
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("qualification_runtime_probe_output_invalid"))?;
    if found.next().is_some() {
        return Err(internal("qualification_runtime_probe_output_invalid"));
    }
    Ok(value.to_owned())
}

fn observe_java(
    selector: &str,
    executable: &str,
    lookup: &MiseExecutableLookup,
    root: &Path,
    expected_sha: &str,
) -> Result<Abi, OrchestratorError> {
    let (observed, output) = run_lookup(
        lookup,
        selector,
        executable,
        root,
        &["-XshowSettings:properties", "-version"],
    )?;
    ensure_same_executable(&observed.sha256, expected_sha)?;
    let arch = prefixed_line(&output, "    os.arch = ")?;
    Ok(Abi::Java {
        version_output: output,
        arch,
    })
}

fn observe_node(
    selector: &str,
    executable: &str,
    lookup: &MiseExecutableLookup,
    root: &Path,
    expected_sha: &str,
    version_output: &str,
) -> Result<Abi, OrchestratorError> {
    let program = "JSON.stringify({version:process.version,moduleAbi:process.versions.modules,napi:process.versions.napi,platform:process.platform,arch:process.arch})";
    let (observed, output) = run_lookup(lookup, selector, executable, root, &["-p", program])?;
    ensure_same_executable(&observed.sha256, expected_sha)?;
    let value: serde_json::Value = serde_json::from_str(output.trim())
        .map_err(|_| internal("qualification_runtime_node_observation_invalid"))?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| internal("qualification_runtime_node_observation_invalid"))
    };
    let runtime_version = field("version")?;
    let module_abi = field("moduleAbi")?;
    let napi = field("napi")?;
    let platform = field("platform")?;
    let arch = field("arch")?;
    Ok(Abi::Node {
        version_output: format!("{version_output}\n{runtime_version}"),
        module_abi,
        napi,
        platform,
        arch,
    })
}

fn observe_tofu(
    selector: &str,
    executable: &str,
    lookup: &MiseExecutableLookup,
    root: &Path,
    expected_sha: &str,
) -> Result<Abi, OrchestratorError> {
    let (observed, output) = run_lookup(lookup, selector, executable, root, &["version", "-json"])?;
    ensure_same_executable(&observed.sha256, expected_sha)?;
    let value: serde_json::Value = serde_json::from_str(&output)
        .map_err(|_| internal("qualification_runtime_tofu_observation_invalid"))?;
    let version = value
        .get("terraform_version")
        .or_else(|| value.get("tofu_version"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| internal("qualification_runtime_tofu_observation_invalid"))?;
    let platform = value
        .get("platform")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| internal("qualification_runtime_tofu_observation_invalid"))?;
    Ok(Abi::OpenTofu {
        version_output: version.to_owned(),
        platform: platform.to_owned(),
    })
}

fn observe_host(inputs: &ToolchainInputs, target: &str) -> Result<Abi, OrchestratorError> {
    let selector = inputs
        .tools
        .first()
        .ok_or_else(|| internal("qualification_runtime_tools_missing"))?;
    let kernel = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map_err(|_| internal("qualification_runtime_kernel_unreadable"))?
            .trim()
            .to_owned()
    } else {
        probe(selector, &["/usr/bin/uname", "-r"])?
            .trim()
            .to_owned()
    };
    if kernel.is_empty() {
        return Err(internal("qualification_runtime_kernel_invalid"));
    }
    let libc = if cfg!(target_os = "linux") && target.ends_with("-gnu") {
        let output = probe(selector, &["/usr/bin/ldd", "--version"])?;
        Some(
            output
                .lines()
                .next()
                .filter(|line| line.to_ascii_lowercase().contains("glibc"))
                .ok_or_else(|| internal("qualification_runtime_libc_invalid"))?
                .to_owned(),
        )
    } else {
        None
    };
    Ok(Abi::Host {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        kernel,
        libc,
    })
}

fn contains_version(text: &str, version: &str) -> bool {
    text.split(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+'))
    })
    .any(|token| token == version || token.strip_prefix('v') == Some(version))
}

#[cfg(test)]
mod tests {
    use super::executable_for;

    #[test]
    fn runtime_observation_uses_jactionlint_and_rejects_retired_linter_names() {
        assert_eq!(
            executable_for("jactionlint").expect("jactionlint"),
            "jactionlint"
        );
        assert!(executable_for("actionlint").is_err());
        assert!(executable_for("zizmor").is_err());
    }
}
