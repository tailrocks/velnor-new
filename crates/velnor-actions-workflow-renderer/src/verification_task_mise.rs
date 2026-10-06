//! Selected, locked, prebuilt-only tool bootstrap for Verification tasks.

use std::collections::BTreeMap;

use crate::build_task_jobs::BuildTaskTool;
use crate::build_task_mise_tools::{
    BOLTFFI_KEY, BOLTFFI_MATCHING_REGEX, exact_rust_version, safe_backend, safe_identity,
    safe_version, selected_mise_files_for, validate_artifact,
};
use crate::steps;
use crate::verification_jobs::{
    INSTALL_VERIFICATION_TOOLS_NAME, RUN_VERIFICATION_TASK_NAME, VerificationTaskPolicy,
};
use crate::{RenderError, mise_setup_step, shell_step};

const JQ_VERSION: &str = "jq-1.8.2";
const MBX_BACKEND: &str = "packslip:github.com/jdx/mr-boxington";
const MBX_TOOL_KEY: &str = "mr-boxington";

pub(crate) fn steps(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<velnor_actions_contract::Step>, RenderError> {
    let env = BTreeMap::from([("MISE_CARGO_BINSTALL_ONLY".to_owned(), "1".to_owned())]);
    let mut rendered = vec![
        steps::checkout_step(checkout_uses)?,
        mise_setup_step(&policy.mise_setup)?,
    ];
    if !policy.selected_tools.is_empty() {
        rendered.push(shell_step(
            INSTALL_VERIFICATION_TOOLS_NAME,
            bash_script(&install_script(policy)?),
            env.clone(),
        )?);
    }
    rendered.push(shell_step(
        RUN_VERIFICATION_TASK_NAME,
        bash_script(&run_script(policy)?),
        env,
    )?);
    Ok(rendered)
}

pub(crate) fn validate_policy(policy: &VerificationTaskPolicy) -> Result<(), RenderError> {
    for digest in [
        &policy.mise_config_sha256,
        &policy.mise_lock_sha256,
        &policy.rust_toolchain_sha256,
    ]
    .into_iter()
    .flatten()
    {
        if !velnor_actions_contract::ids::is_lower_hex_len(digest, 64) {
            return Err(RenderError::InvalidWorkflow(
                "verification_tool_source_sha256".to_owned(),
            ));
        }
    }
    if !policy.selected_tools.is_empty()
        && (policy.mise_config_sha256.is_none() || policy.mise_lock_sha256.is_none())
    {
        return Err(RenderError::InvalidWorkflow(
            "verification_tool_source_missing".to_owned(),
        ));
    }
    if policy.selected_tools.iter().any(|tool| tool.key == "rust")
        && policy.rust_toolchain_sha256.is_none()
    {
        return Err(RenderError::InvalidWorkflow(
            "verification_rust_toolchain_missing".to_owned(),
        ));
    }
    let (mise_os, _) = platform(policy.task.runner);
    let mut previous: Option<&str> = None;
    for tool in &policy.selected_tools {
        if !safe_identity(&tool.key)
            || !safe_version(&tool.version)
            || !safe_backend(&tool.backend)
            || previous.is_some_and(|key| key >= tool.key.as_str())
            || !(tool.os.is_empty() || tool.os == [mise_os])
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_tool_identity:{}",
                tool.key
            )));
        }
        previous = Some(&tool.key);
        validate_tool(tool)?;
    }
    Ok(())
}

fn validate_tool(tool: &BuildTaskTool) -> Result<(), RenderError> {
    if tool.key.starts_with("cargo:")
        || tool.backend.starts_with("cargo:")
        || matches!(
            tool.version.as_str(),
            "latest" | "system" | "ref" | "stable" | "nightly" | "lts"
        )
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "verification_cargo_source_install:{}",
            tool.key
        )));
    }
    if tool.key == "rust" {
        if tool.backend != "core:rust"
            || tool.artifact.is_some()
            || tool.config_options != tool.lock_options
            || !exact_rust_version(&tool.version)
            || tool
                .lock_options
                .keys()
                .any(|key| !matches!(key.as_str(), "components" | "targets"))
            || tool.lock_options.values().any(|value| !safe_option(value))
        {
            return Err(RenderError::InvalidWorkflow(
                "verification_rust_tool_pin".to_owned(),
            ));
        }
        return Ok(());
    }
    let artifact = tool.artifact.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("verification_tool_artifact_missing:{}", tool.key))
    })?;
    let allowed_backend = (tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND)
        || (tool.key == BOLTFFI_KEY && tool.backend == BOLTFFI_KEY)
        || (tool.backend.starts_with("aqua:")
            && tool.backend.len() > "aqua:".len()
            && tool.config_options.is_empty())
        || (tool.key.starts_with("github:")
            && tool.key == tool.backend
            && tool.config_options.is_empty());
    let boltffi_options = BTreeMap::from([(
        "matching_regex".to_owned(),
        BOLTFFI_MATCHING_REGEX.to_owned(),
    )]);
    if !allowed_backend
        || tool.config_options != tool.lock_options
        || (tool.key == BOLTFFI_KEY
            && (tool.config_options != boltffi_options || tool.lock_options != boltffi_options))
        || (tool.key != BOLTFFI_KEY
            && (!tool.config_options.is_empty() || !tool.lock_options.is_empty()))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "verification_tool_backend:{}:{}",
            tool.key, tool.backend
        )));
    }
    validate_artifact(&tool.key, artifact)
}

fn install_script(policy: &VerificationTaskPolicy) -> Result<String, RenderError> {
    let (mise_os, lock_platform) = platform(policy.task.runner);
    let (config, lock) = selected_mise_files_for(
        &policy.selected_tools,
        mise_os,
        lock_platform,
        policy
            .selected_tools
            .iter()
            .any(|tool| tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND),
    )?;
    let root = task_root(policy);
    let mut lines = base_environment(&root);
    lines.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
        "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/home/config\" \"$task_root/home/data\" \"$task_root/home/cache\" \"$task_root/home/state\" \"$task_root/config\" \"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" \"$task_root/cargo\" \"$task_root/rustup\"".to_owned(),
        private_environment(),
        "export MISE_CEILING_PATHS=\"$task_root\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$task_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MISE_CARGO_BINSTALL_ONLY=1".to_owned(),
        ": > \"$task_root/global.toml\"".to_owned(),
        ": > \"$task_root/system.toml\"".to_owned(),
        heredoc("$task_root/mise.toml", "VELNOR_VERIFICATION_MISE_CONFIG", &config),
        heredoc("$task_root/mise.lock", "VELNOR_VERIFICATION_MISE_LOCK", &lock),
        "cd -P \"$task_root\"".to_owned(),
        jq_guard(),
        private_config_chain_check("$task_root/mise.toml"),
        "mise --no-env --locked --no-hooks install --jobs 2".to_owned(),
    ]);
    Ok(lines.join("\n"))
}

fn run_script(policy: &VerificationTaskPolicy) -> Result<String, RenderError> {
    let root = task_root(policy);
    let mut lines = base_environment(&root);
    lines.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "workspace_root=\"$(cd -P \"$GITHUB_WORKSPACE\" && pwd -P)\"".to_owned(),
    ]);
    if policy.selected_tools.is_empty() {
        lines.extend([
            "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
            "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/home/config\" \"$task_root/home/data\" \"$task_root/home/cache\" \"$task_root/home/state\" \"$task_root/config\" \"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" \"$task_root/cargo\" \"$task_root/rustup\"".to_owned(),
            ": > \"$task_root/global.toml\"".to_owned(),
            ": > \"$task_root/system.toml\"".to_owned(),
        ]);
    } else {
        lines.push("test -d \"$task_root\"".to_owned());
    }
    lines.extend([
        private_environment(),
        "export MISE_CEILING_PATHS=\"$workspace_root\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$workspace_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MISE_CARGO_BINSTALL_ONLY=1".to_owned(),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        source_hash_check("mise.toml", policy.mise_config_sha256.as_deref(), policy),
        source_hash_check("mise.lock", policy.mise_lock_sha256.as_deref(), policy),
        source_hash_check(
            "rust-toolchain.toml",
            policy.rust_toolchain_sha256.as_deref(),
            policy,
        ),
        "cd -P \"$workspace_root\"".to_owned(),
        jq_guard(),
        workspace_config_chain_check(policy),
    ]);
    if let Some(mbx) = policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND)
    {
        lines.extend([
            "mbx_path=\"$(mise --no-env --no-hooks which mbx)\"".to_owned(),
            format!(
                "case \"$mbx_path\" in \"$MISE_DATA_DIR/installs/mr-boxington/{}/\"*) ;; *) exit 1 ;; esac",
                mbx.version
            ),
            "test -x \"$mbx_path\" && test ! -L \"$mbx_path\"".to_owned(),
            "mbx_parent=\"$(cd -P \"$(/usr/bin/dirname \"$mbx_path\")\" && pwd -P)\"".to_owned(),
            "test \"$mbx_path\" = \"$mbx_parent/$(/usr/bin/basename \"$mbx_path\")\"".to_owned(),
        ]);
    }
    let locked = if policy.mise_lock_sha256.is_some() {
        "--locked "
    } else {
        ""
    };
    lines.push(format!(
        "mise --no-env {locked}--no-hooks run --skip-tools {}",
        policy.task.mise_task
    ));
    Ok(lines.join("\n"))
}

fn base_environment(root: &str) -> Vec<String> {
    vec![
        "set -euo pipefail".to_owned(),
        "for name in ${!MISE_@}; do unset \"$name\"; done".to_owned(),
        "unset GITHUB_TOKEN GH_TOKEN ACTIONS_RUNTIME_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN"
            .to_owned(),
        "umask 077".to_owned(),
        format!("task_root=\"{root}\""),
        "test -n \"$RUNNER_TEMP\" && test -n \"$GITHUB_WORKSPACE\" && test -n \"$GITHUB_RUN_ID\" && test -n \"$GITHUB_RUN_ATTEMPT\"".to_owned(),
    ]
}

fn private_environment() -> String {
    concat!(
        "export HOME=\"$task_root/home\"\n",
        "export XDG_CONFIG_HOME=\"$task_root/home/config\"\n",
        "export XDG_DATA_HOME=\"$task_root/home/data\"\n",
        "export XDG_CACHE_HOME=\"$task_root/home/cache\"\n",
        "export XDG_STATE_HOME=\"$task_root/home/state\"\n",
        "export MISE_CONFIG_DIR=\"$task_root/config\"\n",
        "export MISE_DATA_DIR=\"$task_root/data\"\n",
        "export MISE_CACHE_DIR=\"$task_root/cache\"\n",
        "export MISE_STATE_DIR=\"$task_root/state\"\n",
        "export CARGO_HOME=\"$task_root/cargo\"\n",
        "export RUSTUP_HOME=\"$task_root/rustup\"\n",
        "export MISE_GLOBAL_CONFIG_FILE=\"$task_root/global.toml\"\n",
        "export MISE_SYSTEM_CONFIG_FILE=\"$task_root/system.toml\"\n",
        "export MISE_AUTO_INSTALL=false\n",
        "export MISE_EXEC_AUTO_INSTALL=false\n",
        "export MISE_TASK_RUN_AUTO_INSTALL=false\n",
        "export MISE_AUTO_ENV=false\n",
        "export MISE_NO_ENV=1\n",
    )
    .to_owned()
}

fn source_hash_check(
    path: &str,
    expected: Option<&str>,
    policy: &VerificationTaskPolicy,
) -> String {
    let (hash_bin, hash_args) = match policy.task.runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("/usr/bin/sha256sum", ""),
        velnor_actions_contract::VerificationRunner::MacosArm64 => ("/usr/bin/shasum", "-a 256"),
    };
    match expected {
        Some(expected) => format!(
            "test -f \"$workspace_root/{path}\" && test ! -L \"$workspace_root/{path}\"\nactual=\"$({hash_bin} {hash_args} \"$workspace_root/{path}\")\"\nactual=\"${{actual%% *}}\"\ntest \"$actual\" = \"{expected}\""
        ),
        None => {
            format!("test ! -e \"$workspace_root/{path}\" && test ! -L \"$workspace_root/{path}\"")
        }
    }
}

fn private_config_chain_check(expected: &str) -> String {
    format!(
        "actual_configs=\"$(mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort)\"\nexpected_configs=\"$(printf '%s\\n' \"{expected}\" \"$task_root/global.toml\" \"$task_root/system.toml\" | LC_ALL=C /usr/bin/sort)\"\ntest \"$actual_configs\" = \"$expected_configs\""
    )
}

fn workspace_config_chain_check(policy: &VerificationTaskPolicy) -> String {
    let mut checks = vec![
        "actual_configs=\"$(mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort)\"".to_owned(),
        "workspace_config_found=false".to_owned(),
        "while IFS= read -r path; do case \"$path\" in \"$workspace_root/mise.toml\") workspace_config_found=true ;; \"$workspace_root/rust-toolchain.toml\"|\"$task_root/global.toml\"|\"$task_root/system.toml\") ;; *) exit 1 ;; esac; done <<< \"$actual_configs\"".to_owned(),
    ];
    checks.push(if policy.mise_config_sha256.is_some() {
        "[[ \"$workspace_config_found\" == true ]]".to_owned()
    } else {
        "[[ \"$workspace_config_found\" == false ]]".to_owned()
    });
    checks.join("\n")
}

fn jq_guard() -> String {
    format!(
        "jq_path=\"$(command -v jq)\"\ntest -x \"$jq_path\"\n\"$jq_path\" --version | /usr/bin/grep -Fqx '{JQ_VERSION}'"
    )
}

fn heredoc(path: &str, delimiter: &str, content: &str) -> String {
    format!("/usr/bin/cat > \"{path}\" <<'{delimiter}'\n{content}\n{delimiter}")
}

fn task_root(policy: &VerificationTaskPolicy) -> String {
    format!(
        "${{RUNNER_TEMP}}/velnor-verification-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-{}",
        policy.task.id
    )
}

fn bash_script(script: &str) -> Vec<String> {
    vec![
        "/bin/bash".to_owned(),
        "-euo".to_owned(),
        "pipefail".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
    ]
}

fn platform(runner: velnor_actions_contract::VerificationRunner) -> (&'static str, &'static str) {
    match runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("linux", "linux-x64"),
        velnor_actions_contract::VerificationRunner::MacosArm64 => ("macos", "macos-arm64"),
    }
}

fn safe_option(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'-'))
}
